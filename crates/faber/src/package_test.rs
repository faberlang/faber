//! Package manifest, library resolution, compile/build, and reader-config behavior tests.
//!
//! Rendered diagnostic text contracts live in `package_text_contract_test.rs`.
//! Shared temp-dir and diagnostic arg helpers stay local until a second consumer
//! needs them.

#[cfg(feature = "mir-metal")]
use super::cmd::compile_package_metal_text;
#[cfg(feature = "hir-rust")]
use super::verify_library_binding_shapes;
use super::{
    ManifestProductEmit, ManifestProductKind, config_with_locale, library_resolver_from_config,
    read_manifest, use_package_compiler_from_args, validate_manifest, verify_library_bindings,
    with_lowered_package_mir,
};
#[cfg(feature = "hir-ts")]
use super::{build_browser_product, build_browser_product_static_assets};
#[cfg(feature = "mir-clif")]
use super::{cmd::compile_clif_package_input, manifestless_file_declares_non_kernel_import};
use super::{cmd::use_package_compiler, faber_analysis_seams, faber_emission_seams};
#[cfg(feature = "hir-rust")]
use super::{
    emit_generated_crate, emit_generated_crate_with_runtime_plan, invoke_cargo_build,
    package_host_selection_diagnostic,
};
use radix::program::analyze::AnalyzedProgram;
#[cfg(feature = "hir-rust")]
use radix::program::discovery::discover_build_layout;
use radix::program::discovery::{BuildLayout, PackageSpec, sanitize_crate_name};
use radix::program::emit::compile_package as compile_package_compiler;
#[cfg(feature = "mir-clif")]
use radix::program::emit::compile_package_clif;
#[cfg(feature = "hir-go")]
use radix::program::emit::compile_package_result;
#[cfg(feature = "hir-rust")]
use radix::program::emit::package_rust_runtime_plan as package_rust_runtime_plan_compiler;
use radix::program::test_source_filter::TestSourceFilter;
use std::collections::BTreeMap;
fn analyze_with_faber_seams(
    config: &Config,
    input: &Path,
) -> Result<AnalyzedProgram, Vec<Diagnostic>> {
    radix::program::analyze::analyze_package(config, input, &faber_analysis_seams())
}

fn check_with_faber_seams(config: &Config, input: &Path) -> Vec<Diagnostic> {
    radix::program::analyze::check_package(config, input, &faber_analysis_seams())
}

fn compile_with_faber_seams(config: &Config, input: &Path) -> radix::module::CompileResult {
    compile_package_compiler(config, input, &faber_emission_seams())
}

#[cfg(feature = "hir-go")]
fn compile_go_with_faber_seams(
    config: &Config,
    input: &Path,
) -> radix::program::emit::PackageCompileResult {
    compile_package_result(config, input, &faber_emission_seams())
}

#[cfg(feature = "hir-rust")]
fn runtime_plan_with_faber_seams(
    config: &Config,
    input: &Path,
) -> Result<super::RustRuntimePlan, Vec<Diagnostic>> {
    package_rust_runtime_plan_compiler(
        config,
        input,
        &faber_analysis_seams(),
        super::rust_runtime_plan_for_package,
    )
}

fn discover_package(input: &Path) -> Result<PackageSpec, Box<Diagnostic>> {
    super::discover_package(input)
}

fn load_with_faber_seams(
    spec: &PackageSpec,
    resolver: &LibraryResolver,
    locale_pack: Option<&radix::module::locale::LocalePack>,
    include_proba: bool,
    filter: Option<&TestSourceFilter>,
) -> Result<Vec<radix::program::PackageFile>, Vec<Diagnostic>> {
    let config = locale_pack
        .cloned()
        .map(|pack| Config::default().with_resolved_locale_pack(pack))
        .unwrap_or_default();
    radix::program::analyze::load_package_with_config(
        spec,
        resolver,
        &config,
        include_proba,
        filter,
        &faber_analysis_seams(),
    )
}

use radix::program::library::{LibraryResolveError, LibraryResolver, ResolvedLibraryModule};
use radix::program::mir::test_support::{fmir_image_test_summary, fmir_text_image_test_summary};

fn build_fmir_image_for_faber(
    config: &Config,
    input: &Path,
    argumenta: &[String],
) -> Result<radix::program::mir::PackageFmirImage, Vec<Diagnostic>> {
    radix::program::mir::build_package_fmir_image(
        config,
        input,
        argumenta,
        &super::mir::faber_mir_seams(),
    )
}

fn build_fmir_text_image_for_faber(
    config: &Config,
    input: &Path,
    argumenta: &[String],
) -> Result<radix::program::mir::PackageFmirTextImage, Vec<Diagnostic>> {
    radix::program::mir::build_package_fmir_text_image(
        config,
        input,
        argumenta,
        &super::mir::faber_mir_seams(),
    )
}

fn build_mir_artifact_for_faber(
    config: &Config,
    input: &Path,
    argumenta: &[String],
) -> Result<radix::program::mir::PackageMirArtifact, Vec<Diagnostic>> {
    radix::program::mir::build_package_mir_artifact(
        config,
        input,
        argumenta,
        &super::mir::faber_mir_seams(),
    )
}

fn run_fmir_built_image_for_faber<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    selection: radix::program::mir::DeviceSelection,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    radix::program::mir::run_package_fmir_built_image(
        config,
        input,
        selection,
        &super::mir::faber_mir_seams(),
        host,
    )
}

fn run_fmir_image_for_faber<H: Host + ?Sized>(
    image: &radix::program::mir::PackageFmirImage,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    radix::program::mir::run_package_fmir_image(image, &super::mir::faber_mir_seams(), host)
}

fn run_fmir_text_image_for_faber<H: Host + ?Sized>(
    image: &radix::program::mir::PackageFmirTextImage,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    radix::program::mir::run_package_fmir_text_image(image, &super::mir::faber_mir_seams(), host)
}

fn run_mir_for_faber<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    radix::program::mir::run_package_mir(config, input, &super::mir::faber_mir_seams(), host)
}

fn run_mir_artifact_for_faber<H: Host + ?Sized>(
    config: &Config,
    artifact: &radix::program::mir::PackageMirArtifact,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    radix::program::mir::run_package_mir_artifact(
        config,
        artifact,
        &super::mir::faber_mir_seams(),
        host,
    )
}
#[cfg(feature = "hir-rust")]
use radix::module::Output;
use radix::module::codegen::Target;
use radix::module::diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase};
use radix::module::driver::Config;
use radix::module::file_interface::FileExportKind;
use radix::module::hir::{
    HirItemKind, LibraryBinding, LibraryItem, LibraryItemKind, LibraryProvider,
};
use radix::module::mir::{BufferHost, Host, MirDiagnosticKind, MirProvider, RunnerError, Value};
use std::fs;
#[cfg(feature = "hir-go")]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(feature = "hir-go")]
use std::process::Stdio;
#[cfg(feature = "hir-go")]
use std::thread;

#[cfg(feature = "hir-ts")]
use super::product::inject_product_failure_at;
use super::test_support::{TestDir, test_temp_dir};

fn diagnostic_has_issue(diag: &Diagnostic, issue: &str) -> bool {
    diag.args.contains(&DiagnosticArg::new("issue", issue))
}

fn diagnostic_has_arg(diag: &Diagnostic, name: &'static str, value: impl Into<String>) -> bool {
    diag.args.contains(&DiagnosticArg::new(name, value))
}

#[test]
fn executed_lane_analysis_target_seam_is_single_valued() {
    // TARGETLANE001 / BUG-F585D081: single-valued precedence is explicit
    // MIR-backed config target > manifest target (including its implicit
    // default) > executed-lane `fmir` only without a manifest. Analysis and
    // execution must consume the same fact.
    let dir = test_temp_dir("lane-seam");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(dir.join("src/main.fab"), "main {}\n").expect("source");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "lane-seam"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"
"#,
    )
    .expect("manifest");

    // A default (HIR-direct) config resolves to the manifest target: the
    // package's declared `fmir` is propagated to the analysis target.
    assert_eq!(
        super::executed_lane_analysis_target(&Config::default(), dir.path()).unwrap(),
        Target::MirFmirBinary,
        "manifest `[build] target = \"fmir\"` must propagate to the analysis target"
    );

    // An explicit MIR-backed config target is authoritative (explicit `-t`).
    assert_eq!(
        super::executed_lane_analysis_target(
            &Config::default().with_target(Target::MirFmir),
            dir.path()
        )
        .unwrap(),
        Target::MirFmir,
        "explicit MIR-backed target must stay authoritative over the manifest"
    );

    // A manifest with no explicit target still owns its implicit `fhir` default.
    // The old `fmir` substitution disagreed with build/check for this manifest.
    let bare = test_temp_dir("lane-seam-bare");
    fs::create_dir_all(bare.join("src")).expect("src");
    fs::write(bare.join("src/main.fab"), "main {}\n").expect("source");
    fs::write(
        bare.join("faber.toml"),
        r#"
[package]
name = "lane-seam-bare"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    assert_eq!(
        super::executed_lane_analysis_target(&Config::default(), bare.path()).unwrap(),
        Target::HirFhir,
        "an existing un-targeted manifest must keep its implicit portable target"
    );
}

#[test]
fn executed_lane_uses_the_manifest_implicit_target() {
    let dir = test_temp_dir("implicit-target-agreement");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(dir.join("src/main.fab"), "main {}\n").expect("source");
    fs::write(
        dir.join("faber.toml"),
        "[package]\nname = \"implicit-target\"\n[paths]\nentry = \"main.fab\"\n",
    )
    .expect("manifest");
    let manifest_target = super::manifest_build_target_for_input(dir.path())
        .expect("valid manifest")
        .expect("manifest exists");
    assert_eq!(manifest_target, Target::HirFhir);
    assert_eq!(
        super::executed_lane_analysis_target(&Config::default(), dir.path()).unwrap(),
        manifest_target,
        "an existing manifest must not be replaced with the executed-lane default"
    );
}

#[test]
fn manifest_declared_targets_keep_their_analysis_identity() {
    let dir = test_temp_dir("declared-target-agreement");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(dir.join("src/main.fab"), "main {}\n").expect("source");
    let manifest_path = dir.join("faber.toml");
    let spellings = Target::ALL
        .iter()
        .map(ToString::to_string)
        .chain(["typescript".to_owned(), "wasm".to_owned()]);
    for spelling in spellings {
        let expected =
            radix::program::manifest::manifest_build_target(Some(&spelling), &manifest_path);
        fs::write(
            &manifest_path,
            format!(
                "[package]\nname = \"declared-target\"\n[paths]\nentry = \"main.fab\"\n[build]\ntarget = \"{spelling}\"\n"
            ),
        )
        .expect("manifest");
        for input in [
            dir.path(),
            manifest_path.as_path(),
            dir.join("src/main.fab").as_path(),
        ] {
            match &expected {
                Ok(target) => {
                    assert_eq!(
                        super::manifest_build_target_for_input(input).unwrap(),
                        Some(*target),
                        "manifest target {spelling}, input {}",
                        input.display()
                    );
                    assert_eq!(
                        super::executed_lane_analysis_target(&Config::default(), input).unwrap(),
                        *target,
                        "executed target {spelling}, input {}",
                        input.display()
                    );
                }
                Err(_) => {
                    assert!(super::manifest_build_target_for_input(input).is_err());
                    assert!(
                        super::executed_lane_analysis_target(&Config::default(), input).is_err()
                    );
                }
            }
        }
    }
}

// Exercise the real process-exiting package command drivers from a child test
// process, without relying on a possibly stale standalone `faber` executable.
#[test]
fn manifest_target_command_probe() {
    let Some(operation) = std::env::var_os("FABER_MANIFEST_TARGET_PROBE") else {
        return;
    };
    let input = std::env::var("FABER_MANIFEST_TARGET_PROBE_INPUT").expect("probe input");
    match operation.to_str().expect("probe operation") {
        "check" => super::cmd_check_package(
            radix::services::CheckCommand {
                input: vec![input],
                package: true,
                permissive: false,
                deny_warnings: false,
                deny_codes: Vec::new(),
                diagnostic_mode: radix::services::DiagnosticMode::Normal,
                diagnostics_locale_pack: None,
                locale: None,
                diagnostics_locale: None,
            },
            None,
            None,
            false,
            None,
        ),
        "build" | "build-explicit" => super::cmd_build(
            radix::services::BuildCommand {
                input,
                out_dir: PathBuf::new(),
                package: true,
                release: false,
                target: Target::HirFhir,
                target_explicit: operation.to_str() == Some("build-explicit"),
                deny_warnings: false,
                deny_codes: Vec::new(),
                locale: None,
                diagnostics_locale: None,
                diagnostic_mode: radix::services::DiagnosticMode::Normal,
            },
            false,
            false,
            false,
        ),
        other => panic!("unknown probe operation {other}"),
    }
}

#[cfg(feature = "hir-fhir")]
#[test]
fn package_check_refuses_the_manifest_target_build_refuses() {
    let dir = test_temp_dir("check-build-target-refusal");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(dir.join("src/main.fab"), "main {}\n").expect("source");
    fs::write(
        dir.join("faber.toml"),
        "[package]\nname = \"bad-target\"\n[paths]\nentry = \"main.fab\"\n[build]\nkind = \"lib\"\ntarget = \"ada\"\ntargets = [\"rust\"]\n[library]\nprovider = \"bad-target\"\n",
    )
    .expect("manifest");
    let mut errors = Vec::new();
    for op in ["check", "build", "build-explicit"] {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "package::tests::manifest_target_command_probe",
                "--nocapture",
            ])
            .env("FABER_MANIFEST_TARGET_PROBE", op)
            .env("FABER_MANIFEST_TARGET_PROBE_INPUT", dir.path())
            .output()
            .expect("run package command probe");
        assert_eq!(output.status.code(), Some(78), "{op}: {output:?}");
        let stderr = String::from_utf8(output.stderr).expect("diagnostic UTF-8");
        assert!(
            stderr.contains("build.target 'ada' is not a build target"),
            "{stderr}"
        );
        errors.push(stderr);
    }
    assert_eq!(
        errors[0], errors[1],
        "check/build must refuse the same manifest fact"
    );
    assert_eq!(
        errors[0], errors[2],
        "an explicit build must also fail closed"
    );

    // A valid explicit override still wins; manifest validation must not turn
    // the manifest into an override of an intentional CLI target.
    fs::write(
        dir.join("faber.toml"),
        "[package]\nname = \"valid-target\"\n[paths]\nentry = \"main.fab\"\n[build]\ntarget = \"fmir\"\n",
    )
    .expect("valid manifest");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "package::tests::manifest_target_command_probe",
            "--nocapture",
        ])
        .env("FABER_MANIFEST_TARGET_PROBE", "build-explicit")
        .env("FABER_MANIFEST_TARGET_PROBE_INPUT", dir.path())
        .output()
        .expect("run explicit build probe");
    assert!(output.status.success(), "valid explicit build: {output:?}");
    assert!(dir.join("target/faber-fhir/package.fhirpkg").is_file());
}

#[derive(Debug)]
struct ExitPanic(i32);

#[derive(Default)]
struct ExitRecordingHost {
    buffer: BufferHost,
}

impl ExitRecordingHost {
    fn with_argumenta(argumenta: Vec<String>) -> Self {
        Self {
            buffer: BufferHost::with_argumenta(argumenta),
        }
    }
}

impl Host for ExitRecordingHost {
    fn scribe(&mut self, kind: MirDiagnosticKind, text: &str) -> Result<(), RunnerError> {
        self.buffer.scribe(kind, text)
    }

    fn read_line(&mut self) -> Result<Option<String>, RunnerError> {
        self.buffer.read_line()
    }

    fn read_all(&mut self) -> Result<Option<String>, RunnerError> {
        self.buffer.read_all()
    }

    fn abort(&mut self, reason: &str) -> ! {
        self.buffer.abort(reason)
    }

    fn provider(&mut self, provider: &MirProvider) -> Result<Value, RunnerError> {
        self.buffer.provider(provider)
    }

    fn exit(&mut self, code: i32) -> ! {
        std::panic::panic_any(ExitPanic(code));
    }

    fn argumenta(&self) -> Result<&[String], RunnerError> {
        self.buffer.argumenta()
    }
}

/// The product-default English code pack (en = authority). The transitive
/// loader tests below call `load_package`/`load_package_with_config`
/// directly; they must pass the same default pack the `compile_package*` /
/// `analyze_package` routes apply — not the pack-less (registry-direct)
/// lexing `load_package` would select.
fn english_pack() -> radix::module::locale::LocalePack {
    let path = radix::module::locale::workspace_locale_pack_path("en");
    radix::module::locale::LocalePack::from_toml_path(path).expect("load English locale pack")
}

struct CoreutilsLikePackage {
    _workspace: TestDir,
    package: PathBuf,
    src: PathBuf,
    common: PathBuf,
}

fn coreutils_like_package(label: &str) -> CoreutilsLikePackage {
    let workspace = test_temp_dir(label);
    let package = workspace.join("packages").join("echo");
    let src = package.join("src");
    let common = workspace.join("common").join("gnu");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(&common).expect("create shared common");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "echo"

[paths]
source = "src"
entry = "main.fab"

[paths.templates]
gnu = "../../common/gnu"
"#,
    )
    .expect("write manifest");
    CoreutilsLikePackage {
        _workspace: workspace,
        package,
        src,
        common,
    }
}

fn library_path_eq(actual: &[String], expected: &[&str]) -> bool {
    actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
}

fn library_binding_by_module<'a>(
    analysis: &'a radix::module::driver::AnalyzedModule,
    module_path: &[&str],
) -> &'a LibraryBinding {
    analysis
        .libraries
        .bindings
        .values()
        .find(|binding| library_path_eq(&binding.identity.module_path, module_path))
        .expect("library binding")
}

fn library_item_by_export<'a>(
    analysis: &'a radix::module::driver::AnalyzedModule,
    module_path: &[&str],
    exported_name: &str,
    kind: LibraryItemKind,
) -> &'a LibraryItem {
    analysis
        .libraries
        .items
        .values()
        .find(|item| {
            library_path_eq(&item.identity.module_path, module_path)
                && item.exported_name == exported_name
                && item.kind == kind
        })
        .expect("library item")
}

fn write_zh_locale_pack(root: &Path, name: &str) -> PathBuf {
    let reader = root.join("locale");
    let exemplars = reader.join("exemplars");
    fs::create_dir_all(&exemplars).expect("create reader exemplars");
    fs::write(
        exemplars.join("salve-munde.zh-Hans.fab"),
        "入口 { 输出 \"ok\" }",
    )
    .expect("write exemplar");
    let pack = reader.join(name);
    fs::write(
        &pack,
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
incipit = "入口"
nota = "输出"
functio = "函数"

[diagnostics.LOCALE002]
message = "suggestion: {suggestion}"
help = "use {suggestion}"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("write reader pack");
    pack
}

#[cfg(feature = "hir-rust")]
fn compile_emit_build_run(entry: &Path) -> String {
    let code = compile_package_rust_code(entry);
    let layout = discover_build_layout(entry).expect("layout");
    emit_generated_crate(&layout, &code, None).expect("emit generated crate");
    let binary = invoke_cargo_build(&layout, false).expect("cargo build");
    let run = Command::new(binary).output().expect("run generated binary");
    assert!(run.status.success(), "generated binary failed: {run:?}");
    String::from_utf8(run.stdout).expect("stdout utf8")
}

#[cfg(feature = "hir-rust")]
fn compile_package_rust_code(entry: &Path) -> String {
    let result = compile_with_faber_seams(&Config::default(), entry);
    assert!(
        result.success(),
        "expected package compile success, got {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    output.code
}

#[test]
fn compile_package_reports_unresolved_external_imports() {
    let dir = test_temp_dir("external-import");
    let entry = dir.join("main.fab");
    fs::write(&entry, "import from \"lodash\" map\nmain { print \"x\" }").expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "package_import_unsupported_path"))
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_accepts_empty_source_as_noop() {
    let dir = test_temp_dir("empty-source");
    let entry = dir.join("main.fab");
    fs::write(&entry, "").expect("write empty entry");
    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "empty source should compile as a no-op program, got {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_rust_emit_contains_arm_cape_throw() {
    let dir = test_temp_dir("package-cape-emit");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
fn classify(int x) → string {
    if x ≻ 0 {
        throw "positive"
    } catch e {
        return "caught"
    }
    return "zero"
}

main {
    print classify(1)
}
"#,
    )
    .expect("write cape source");

    let code = compile_package_rust_code(&entry);
    let start = code
        .find("fn classify(p_x: i64) -> String")
        .expect("classify signature");
    let function = code
        .get(start..code.find("\nfn main").expect("main signature"))
        .expect("classify function span");
    assert!(
        !function.contains("fn classify(p_x: i64) -> Result") && !function.contains(".expect("),
        "package emit must contain the arm throw instead of exposing a Result signature/expect:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_rust_cli_emit_contains_arm_cape_throw() {
    let dir = test_temp_dir("package-cape-cli-emit");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
fn classify(int x) → string {
    if x ≻ 0 {
        throw "positive"
    } catch e {
        return "caught"
    }
    return "zero"
}

@ cli "tool"
main args args {
    print classify(1)
}
"#,
    )
    .expect("write CLI cape source");

    let code = compile_package_rust_code(&entry);
    let start = code
        .find("fn classify(p_x: i64) -> String")
        .expect("CLI classify signature");
    let function = code
        .get(start..code.find("\nfn main").expect("CLI main signature"))
        .expect("CLI classify function span");
    assert!(
        !function.contains("fn classify(p_x: i64) -> Result") && !function.contains(".expect("),
        "CLI package emit must preserve the contained arm-cape signature:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_library_imports_without_local_modules() {
    let dir = test_temp_dir("norma-json-import");
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "norma-json-import"
version = "0.1.0"

[paths]
entry = "src/main.fab"
"#,
    )
    .expect("write manifest");
    let entry = dir.join("src").join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:json/solve" solve as json_solve

main {
  do {
    let json parsed ← json_solve.parse("{}")
    print parsed
  }
  catch err {
    print err
  }
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:json package compile success, got {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn parse"));
    assert!(!output.code.contains("norma::json::parse"));
    assert!(!output.code.contains("crate::norma::json"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn build_package_resolves_norma_json_facade_roundtrip() {
    let dir = test_temp_dir("norma-json-build-roundtrip");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:json" json

main {
  do {
    let json arr ← json.parse("[1]")
    let string arr_wire ← json.stringify(arr)
    let json obj ← json.parse("{\"a\":1}")
    let string obj_wire ← json.stringify(obj)
    print arr_wire
    print obj_wire
  }
  catch err {
    print err
  }
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:json package compile success, got {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("pub mod json"));
    assert!(output.code.contains("fn parse"));
    assert!(output.code.contains("fn stringify"));
    assert!(output.code.contains("crate::json::solve::parse"));
    assert!(output.code.contains("crate::json::serialize::stringify"));
    assert!(output.code.contains("pub mod solve"));
    assert!(output.code.contains("pub mod serialize"));
    assert!(output.code.contains("fn parse_value"));
    assert!(output.code.contains("fn encode"));
    assert!(
        !output
            .code
            .contains("deferred pending runtime codec dispatch")
    );
    assert!(!output.code.contains("faber::Json::parse"));
    assert!(!output.code.contains(".to_wire()"));
    assert!(!output.code.contains("norma::json::parse"));
    assert!(!output.code.contains("crate::norma::json"));
}

#[test]
fn compile_package_rejects_direct_text_json_wire_conversions() {
    let dir = test_temp_dir("direct-json-wire-conversion");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
main {
  let string wire ← "{}"
  let json parsed ← wire ↦ json
  let string out ← parsed ↦ string
  print out
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "unsupported_conversio")),
        "expected direct text/json conversion rejection, got {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-rust")]
#[test]
#[ignore = "json runtime e2e builds generated Rust; run with ignored integration checks"]
fn run_package_validates_norma_json_object_array_roundtrip() {
    let dir = test_temp_dir("norma-json-run-roundtrip");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:json" json

@ json
class Payload {
  mut string name
  mut list<int> counts
}

main {
  do {
    let string original ← "{\"counts\":[1,2],\"name\":\"Ada\"}"
    let json parsed ← json.parse(original)
    let Payload payload ← parsed ↦ Payload
    let json encoded ← json.parse(original)
    let string wire ← json.stringify(encoded)
    let json reparsed ← json.parse(wire)
    let Payload roundtrip ← reparsed ↦ Payload
    print payload.name
    print payload.counts.longitudo()
    print wire
    print roundtrip.name
    print roundtrip.counts.longitudo()
  }
  catch err {
    print err
  }
}
"#,
    )
    .expect("write entry");

    let stdout = compile_emit_build_run(&entry);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["Ada", "2", r#"{"counts":[1,2],"name":"Ada"}"#, "Ada", "2"]
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_prefers_locked_norma_interfaces_over_library_home_without_dependency() {
    let dir = test_temp_dir("locked-norma-platform-default");
    let src = dir.join("src");
    let locked_interfaces = dir.join("store").join("norma/0.1.0/interfaces");
    let library_home = dir.join("library-home");
    fs::create_dir_all(&src).expect("create src");
    fs::create_dir_all(locked_interfaces.join("fs")).expect("create locked interfaces");
    fs::create_dir_all(library_home.join("norma/src/fs")).expect("create fallback interfaces");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "locked-norma-platform-default"
version = "0.1.0"

[paths]
entry = "src/main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(
        dir.join("faber.lock"),
        format!(
            r#"
[[package]]
name = "norma"
version = "0.1.0"
source = "path"
package_root = "{}"
kind = "source"
target_language = "rust"
target_triple = "host"
target_manifest = "{}"
interface_root = "{}"
artifact = ""
crate = "norma"
rustc = ""
"#,
            dir.join("store/norma/0.1.0").display(),
            dir.join("store/norma/0.1.0/targets/cista.toml").display(),
            locked_interfaces.display(),
        ),
    )
    .expect("write lock");
    fs::write(
        locked_interfaces.join("fs/path.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio locked_label() → textus {
  redde "locked"
}
"#,
    )
    .expect("write locked norma interface");
    fs::write(
        library_home.join("norma/src/fs/path.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio fallback_label() → textus {
  redde "fallback"
}
"#,
    )
    .expect("write fallback norma interface");
    let entry = src.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs/path" path

main {
  print path.locked_label()
}
"#,
    )
    .expect("write entry");

    let result =
        compile_with_faber_seams(&Config::default().with_library_home(library_home), &entry);
    assert!(
        result.success(),
        "expected locked norma platform default compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("locked_label"));
    assert!(!output.code.contains("fallback_label"));
}

#[cfg(feature = "hir-rust")]
#[test]
#[ignore = "requires a rustfmt binary on PATH; manual formatting proof"]
fn package_emits_rustfmt_clean_generated_main_for_multifile_package() {
    let dir = test_temp_dir("rustfmt-clean-multifile-package");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r#"
@ public
fn label() → string {
  return "ok"
}
"#,
    )
    .expect("write module");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected multifile package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    let layout = discover_build_layout(&entry).expect("layout");
    emit_generated_crate(&layout, &output.code, None).expect("emit generated crate");

    let rustfmt = Command::new("rustfmt")
        .args(["--edition", "2021", "--check"])
        .arg(&layout.generated_rust_entry)
        .output()
        .expect("rustfmt");
    assert!(
        rustfmt.status.success(),
        "generated package main.rs must be rustfmt-clean\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rustfmt.stdout),
        String::from_utf8_lossy(&rustfmt.stderr)
    );
}

#[test]
fn package_mir_linking_executes_two_file_package_without_rust_emit() {
    let dir = test_temp_dir("package-mir-linking");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r#"
main {
  print "module entry must not run"
}

@ public
fn label() → int {
  return 7
}
"#,
    )
    .expect("write module");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected package MIR run success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["7".to_owned()]);
}

#[test]
fn package_mir_cli_root_entry_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-root");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
main args args {
  print "cli root"
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected CLI package MIR run success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["cli root".to_owned()]);
}

#[test]
fn package_mir_artifact_runs_hello_world_without_rust_emit() {
    let dir = test_temp_dir("package-mir-artifact-hello");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
main {
  print "Salve, Munde!"
}
"#,
    )
    .expect("write entry");

    let artifact = build_mir_artifact_for_faber(&Config::default(), &entry, &[])
        .expect("build package MIR artifact");
    assert!(artifact.root.ends_with("target/faber-mir"));
    assert!(artifact.manifest_path.exists());
    assert!(!dir.join("target/faber/Cargo.toml").exists());

    let mut host = BufferHost::default();
    let result = run_mir_artifact_for_faber(&Config::default(), &artifact, &mut host);

    assert!(
        result.is_ok(),
        "expected package MIR artifact run success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["Salve, Munde!".to_owned()]);
    assert!(!dir.join("target/faber/Cargo.toml").exists());
}

#[test]
fn package_mir_artifact_manifest_declares_runtime_requirements() {
    let dir = test_temp_dir("package-mir-artifact-runtime-requirements");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
  mut list<string> partes ← [args.root, "nested"]
  print fs.join(partes)
}
"#,
    )
    .expect("write entry");

    let artifact = build_mir_artifact_for_faber(
        &Config::default(),
        &entry,
        &[dir.to_string_lossy().into_owned()],
    )
    .expect("build package MIR artifact");
    let manifest = fs::read_to_string(&artifact.manifest_path).expect("read manifest");

    assert!(manifest.contains("[runtime]"), "manifest:\n{manifest}");
    for requirement in [
        "requirement = \"host:argv\"",
        "requirement = \"host:stdout\"",
    ] {
        assert!(
            manifest.contains(requirement),
            "missing {requirement} in manifest:\n{manifest}"
        );
    }
    // core-norma N2 (decision log 2026-10-12): "`solum:*` is not host-required
    // at plan time." `norma:fs` runs through the `faber-leaf` fs leaf, so the
    // retired provider-route requirements must not be declared.
    for retired in ["host:fs", "kernel:solum."] {
        assert!(
            !manifest.contains(retired),
            "retired fs provider requirement {retired} in manifest:\n{manifest}"
        );
    }
}

#[test]
fn package_mir_artifact_manifest_layout_is_deterministic() {
    let dir = test_temp_dir("package-mir-artifact-deterministic-manifest");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./message" message

main {
  print message.text()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("message.fab"),
        r#"
@ public
fn text() → string {
  return "stable"
}
"#,
    )
    .expect("write module");

    let first = build_mir_artifact_for_faber(&Config::default(), &entry, &[])
        .expect("first package MIR artifact build");
    let first_manifest = fs::read_to_string(&first.manifest_path).expect("read first manifest");
    let second = build_mir_artifact_for_faber(&Config::default(), &entry, &[])
        .expect("second package MIR artifact build");
    let second_manifest = fs::read_to_string(&second.manifest_path).expect("read second manifest");

    assert_eq!(first.manifest_path, dir.join("target/faber-mir/image.toml"));
    assert_eq!(first.manifest_path, second.manifest_path);
    assert_eq!(first_manifest, second_manifest);
    assert_eq!(
        first_manifest,
        r#"version = 8
target = "scena"
entry = "main.fab"
entry_function = "run_entry"

[runtime]
requirement = "host:stdout"

[sources]
file = "main.fab"
file = "message.fab"
"#
    );
}

#[test]
fn package_mir_artifact_harness_hello_world() {
    artifact_harness_run_test("hello-world", |_case| {
        let dir = test_temp_dir("artifact-harness-hello");
        let input = dir.join("main.fab");
        fs::write(&input, "main { print \"Salve, Munde!\" }").expect("write hello");
        ArtifactHarnessCaseInner {
            _dir: dir,
            name: "hello-world",
            input,
            argumenta: Vec::new(),
            expected_stdout: vec!["Salve, Munde!".to_owned()],
        }
    });
}

#[test]
fn package_mir_artifact_harness_multifile() {
    artifact_harness_run_test("multi-file", |_case| {
        let dir = test_temp_dir("artifact-harness-multifile");
        let input = dir.join("main.fab");
        fs::write(
            &input,
            r#"
import from "./message" message

main {
  print message.text()
}
"#,
        )
        .expect("write entry");
        fs::write(
            dir.join("message.fab"),
            r#"
@ public
fn text() → string {
  return "multi"
}
"#,
        )
        .expect("write module");
        ArtifactHarnessCaseInner {
            _dir: dir,
            name: "multi-file",
            input,
            argumenta: Vec::new(),
            expected_stdout: vec!["multi".to_owned()],
        }
    });
}

#[test]
fn package_mir_artifact_harness_cli_argv() {
    artifact_harness_run_test("cli-argv", |_case| {
        let dir = test_temp_dir("artifact-harness-cli");
        let input = dir.join("main.fab");
        fs::write(
            &input,
            r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#,
        )
        .expect("write cli");
        ArtifactHarnessCaseInner {
            _dir: dir,
            name: "cli-argv",
            input,
            argumenta: vec!["Ian".to_owned()],
            expected_stdout: vec!["Ian".to_owned()],
        }
    });
}

#[test]
fn package_mir_artifact_harness_coreutils_touch() {
    artifact_harness_run_test("coreutils-touch", |_case| {
        let dir = test_temp_dir("artifact-harness-coreutils-touch");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create touch src");
        fs::write(
            dir.join("faber.toml"),
            r#"
[package]
name = "touch-artifact-floor"

[locale]
locale = "la"

[paths]
source = "src"
entry = "main.fab"
"#,
        )
        .expect("write manifest");
        fs::write(
            src.join("main.fab"),
            r#"+++
locale = "la"
+++
importa ex "norma:fs" solum
importa ex "norma:fault" fault

functio tange_paths(lista<textus> paths) → vacuum ⇥ fault.IoFault {
    itera ex paths sit path {
        solum.tange(path)
    }
}

@ cli "touch"
@ descriptio "GNU coreutils touch create-file parity exemplum"
@ operandus ceteri textus paths
incipit argumenta args {
    tange_paths(args.paths)
}
"#,
        )
        .expect("write touch source");
        let input = dir.to_path_buf();
        let touched = dir.join("created.txt");
        ArtifactHarnessCaseInner {
            _dir: dir,
            name: "coreutils-touch",
            input,
            argumenta: vec![touched.to_string_lossy().into_owned()],
            expected_stdout: Vec::new(),
        }
    });
}

struct ArtifactHarnessCaseInner {
    _dir: TestDir,
    name: &'static str,
    input: PathBuf,
    argumenta: Vec<String>,
    expected_stdout: Vec<String>,
}

fn artifact_harness_run_test(
    _case_name: &'static str,
    setup: impl FnOnce(&'static str) -> ArtifactHarnessCaseInner,
) {
    let case = setup(_case_name);
    let config = Config::default();
    let artifact = build_mir_artifact_for_faber(&config, &case.input, &case.argumenta)
        .unwrap_or_else(|diagnostics| {
            panic!(
                "{} build-or-link failed: {:?}",
                case.name,
                diagnostics
                    .iter()
                    .map(|diag| (diag.code, diag.issue()))
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        artifact.manifest_path.exists(),
        "{} manifest path missing: {}",
        case.name,
        artifact.manifest_path.display()
    );
    let mut host = BufferHost::with_argumenta(case.argumenta.clone());
    run_mir_artifact_for_faber(&config, &artifact, &mut host).unwrap_or_else(|diagnostics| {
        panic!(
            "{} run failed: {:?}",
            case.name,
            diagnostics
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        host.stdout_lines, case.expected_stdout,
        "{} output mismatch",
        case.name
    );
    if case.name == "coreutils-touch" {
        let touched = case.input.join("created.txt");
        assert!(
            touched.is_file(),
            "coreutils-touch did not create {}",
            touched.display()
        );
    }
}

#[test]
fn package_mir_artifact_rejects_malformed_manifest() {
    let dir = test_temp_dir("package-mir-artifact-bad-manifest");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
main {
  print "must not run"
}
"#,
    )
    .expect("write entry");

    let artifact = build_mir_artifact_for_faber(&Config::default(), &entry, &[])
        .expect("build package MIR artifact");
    fs::write(&artifact.manifest_path, "version = 999\n").expect("corrupt manifest");

    let mut host = BufferHost::default();
    let diagnostics = run_mir_artifact_for_faber(&Config::default(), &artifact, &mut host)
        .expect_err("malformed package MIR artifact should fail closed");

    assert!(
        diagnostics.iter().any(|diag| diagnostic_has_issue(
            diag,
            "package_mir_artifact_manifest_metadata_missing"
        ))
    );
    let manifest_file = artifact.manifest_path.to_string_lossy();
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.phase == DiagnosticPhase::Tool && diag.file == manifest_file.as_ref())
    );
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

#[test]
fn package_build_scena_target_writes_artifact_without_rust_emit() {
    let dir = test_temp_dir("package-build-scena");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
main {
  print "Salve, Munde!"
}
"#,
    )
    .expect("write entry");

    let config = Config::default().with_target(Target::MirScena);
    let artifact =
        build_mir_artifact_for_faber(&config, &entry, &[]).expect("build scena package artifact");

    assert_eq!(
        artifact.manifest_path,
        dir.join("target/faber-mir/image.toml")
    );
    assert!(artifact.manifest_path.exists());
    assert!(!dir.join("target/faber/Cargo.toml").exists());
}

#[test]
fn package_fmir_text_image_runs_after_source_is_removed() {
    let dir = test_temp_dir("package-fmir-text-source-independent");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
main {
  print "Salve, Munde!"
}
"#,
    )
    .expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read fmir text");

    assert_eq!(
        image.image_path,
        dir.join("target/faber-mir/image.fmir.txt")
    );
    assert!(
        image_text.contains("target = \"fmir-text\""),
        "image:\n{image_text}"
    );
    assert!(image_text.contains("[program]"), "image:\n{image_text}");
    assert!(
        !image_text.contains("source_text"),
        "image must not embed Faber source:\n{image_text}"
    );
    assert!(!dir.join("target/faber/Cargo.toml").exists());

    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::default();
    run_fmir_text_image_for_faber(&image, &mut host).expect("run fmir-text image without source");

    assert_eq!(host.stdout_lines, vec!["Salve, Munde!".to_owned()]);
    assert!(!dir.join("target/faber/Cargo.toml").exists());
}

#[test]
fn package_fmir_text_image_multifile_is_deterministic_and_source_independent() {
    let dir = test_temp_dir("package-fmir-text-multifile");
    let entry = dir.join("main.fab");
    let module = dir.join("message.fab");
    fs::write(
        &entry,
        r#"
import from "./message" message

main {
  print message.text()
}
"#,
    )
    .expect("write entry");
    fs::write(
        &module,
        r#"
@ public
fn text() → string {
  return "multi"
}
"#,
    )
    .expect("write module");

    let config = Config::default().with_target(Target::MirFmir);
    let first =
        build_fmir_text_image_for_faber(&config, &entry, &[]).expect("first fmir-text build");
    let first_image = fs::read_to_string(&first.image_path).expect("read first image");
    let second =
        build_fmir_text_image_for_faber(&config, &entry, &[]).expect("second fmir-text build");
    let second_image = fs::read_to_string(&second.image_path).expect("read second image");

    assert_eq!(first.image_path, second.image_path);
    assert_eq!(first_image, second_image);
    assert!(
        first_image.contains("\"main.fab\""),
        "image:\n{first_image}"
    );
    assert!(
        first_image.contains("\"message.fab\""),
        "image:\n{first_image}"
    );

    fs::write(&module, "fn text() → string { return \"changed\" }").expect("mutate module");
    fs::write(&entry, "main { print \"changed\" }").expect("mutate entry");

    let mut host = BufferHost::default();
    run_fmir_text_image_for_faber(&first, &mut host).expect("run fmir-text image after mutation");

    assert_eq!(host.stdout_lines, vec!["multi".to_owned()]);
}

#[test]
fn package_fmir_text_image_records_source_hashes() {
    let dir = test_temp_dir("package-fmir-text-source-hashes");
    let entry = dir.join("main.fab");
    let module = dir.join("message.fab");
    fs::write(
        &entry,
        r#"
import from "./message" message

main {
  print message.text()
}
"#,
    )
    .expect("write entry");
    fs::write(
        &module,
        r#"
@ public
fn text() → string {
  return "multi"
}
"#,
    )
    .expect("write module");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");

    assert!(
        image_text.contains("[[sources.source]]"),
        "image:\n{image_text}"
    );
    assert_eq!(
        image_text.matches("hash = \"fnv64:").count(),
        2,
        "image:\n{image_text}"
    );
}

#[test]
fn package_mir_artifacts_preserve_quoted_source_paths() {
    let dir = test_temp_dir("package-mir-quoted-source");
    let entry = dir.join("main\"quoted.fab");
    fs::write(&entry, "main { print \"quoted\" }").expect("write quoted entry");

    let config = Config::default().with_target(Target::MirFmir);
    let artifact = build_mir_artifact_for_faber(&config, &entry, &[]).expect("build manifest");
    let manifest_text = fs::read_to_string(&artifact.manifest_path).expect("read manifest");
    let manifest: toml::Value = toml::from_str(&manifest_text).expect("parse manifest");
    assert_eq!(
        manifest
            .get("entry")
            .and_then(toml::Value::as_str)
            .expect("manifest entry"),
        "main\"quoted.fab"
    );

    let image = build_fmir_text_image_for_faber(&config, &entry, &[]).expect("build fmir-text");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");
    let image: toml::Value = toml::from_str(&image_text).expect("parse image");
    let source_file = image
        .get("sources")
        .and_then(|sources| sources.get("source"))
        .and_then(toml::Value::as_array)
        .and_then(|sources| sources.first())
        .and_then(|source| source.get("file"))
        .and_then(toml::Value::as_str)
        .expect("first source file");
    assert_eq!(source_file, "main\"quoted.fab");
}

#[test]
fn package_fmir_text_image_rejects_bad_version_without_source_fallback() {
    let dir = test_temp_dir("package-fmir-text-bad-version");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"must not run\" }").expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let mut image_text = fs::read_to_string(&image.image_path).expect("read image");
    image_text = rewrite_text_image_version(&image_text, 999);
    fs::write(&image.image_path, image_text).expect("write corrupt image");
    fs::remove_file(&entry).expect("remove source after corrupting image");

    let mut host = BufferHost::default();
    let diagnostics = run_fmir_text_image_for_faber(&image, &mut host)
        .expect_err("bad version should fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_text_image_version_unsupported")
            && diagnostic_has_arg(diag, "actual", "999")
            && diagnostic_has_arg(diag, "expected", "8")
    }));
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

#[test]
fn package_fmir_text_image_rejects_unknown_records_without_source_fallback() {
    let dir = test_temp_dir("package-fmir-text-unknown-record");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"must not run\" }").expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let mut image_text = fs::read_to_string(&image.image_path).expect("read image");
    image_text.push_str("\nunsupported = true\n");
    fs::write(&image.image_path, image_text).expect("write image with unknown record");
    fs::remove_file(&entry).expect("remove source after corrupting image");

    let mut host = BufferHost::default();
    let diagnostics = run_fmir_text_image_for_faber(&image, &mut host)
        .expect_err("unknown fmir-text records should fail closed");

    assert!(
        diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "fmir_text_image_parse_failed"))
    );
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

#[test]
fn package_fmir_text_image_rejects_unknown_runtime_requirement_without_source_fallback() {
    let dir = test_temp_dir("package-fmir-text-unknown-runtime-requirement");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"must not run\" }").expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");
    let image_text = image_text.replacen(
        r#"requirement = ["host:stdout"]"#,
        r#"requirement = ["host:teleport"]"#,
        1,
    );
    assert!(
        image_text.contains(r#"requirement = ["host:teleport"]"#),
        "runtime requirement replacement must affect image:\n{image_text}"
    );
    fs::write(&image.image_path, image_text).expect("write corrupt runtime requirement");
    fs::remove_file(&entry).expect("remove source after corrupting image");

    let mut host = BufferHost::default();
    let diagnostics = run_fmir_text_image_for_faber(&image, &mut host)
        .expect_err("unknown fmir-text runtime requirement should fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_runtime_requirement_unsupported")
            && diagnostic_has_arg(diag, "format", "fmir-text")
            && diagnostic_has_arg(diag, "requirement", "host:teleport")
    }));
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

#[test]
fn package_fmir_text_image_rejects_bad_type_metadata_without_source_fallback() {
    let dir = test_temp_dir("package-fmir-text-bad-type-metadata");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &["Ian".to_owned()],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");
    let image_text = image_text.replacen(r#"{ Primitive = "Textus" }"#, r"{ Struct = 999999 }", 1);
    fs::write(&image.image_path, image_text).expect("write corrupt type metadata");
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::default();
    let diagnostics = run_fmir_text_image_for_faber(&image, &mut host)
        .expect_err("bad type metadata should fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_image_type_metadata_invalid")
            && diagnostic_has_arg(diag, "format", "fmir-text")
    }));
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

#[test]
fn package_fmir_text_image_cli_operand_uses_runtime_argument_after_source_is_removed() {
    let dir = test_temp_dir("package-fmir-text-cli-runtime-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");
    assert!(
        !image_text.contains("runtime-value"),
        "image must not bake runtime argv:\n{image_text}"
    );
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::with_argumenta(vec!["runtime-value".to_owned()]);
    run_fmir_text_image_for_faber(&image, &mut host).expect("run CLI fmir-text image");

    assert_eq!(host.stdout_lines, vec!["runtime-value".to_owned()]);
}

#[test]
fn package_fmir_text_image_cli_rest_text_operand_uses_runtime_arguments() {
    let dir = test_temp_dir("package-fmir-text-cli-rest-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand rest string files
main args args {
  let list<string> files ← args.files
  print files.length()
}
"#,
    )
    .expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");
    assert!(
        !image_text.contains("alpha"),
        "image must not bake runtime rest argv:\n{image_text}"
    );
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::with_argumenta(vec!["alpha".to_owned(), "beta".to_owned()]);
    run_fmir_text_image_for_faber(&image, &mut host).expect("run rest CLI fmir-text image");

    assert_eq!(host.stdout_lines, vec!["2".to_owned()]);
}

#[test]
fn package_fmir_image_cli_operand_uses_runtime_argument_after_source_is_removed() {
    let dir = test_temp_dir("package-fmir-cli-runtime-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    let image = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    )
    .expect("build fmir image");
    let image_bytes = fs::read(&image.image_path).expect("read image");
    assert_eq!(image.image_path, dir.join("target/faber-mir/image.fmir"));
    assert!(
        !bytes_contain(&image_bytes, b"runtime-value"),
        "image must not bake runtime argv"
    );
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::with_argumenta(vec!["runtime-value".to_owned()]);
    run_fmir_image_for_faber(&image, &mut host).expect("run CLI fmir image");

    assert_eq!(host.stdout_lines, vec!["runtime-value".to_owned()]);
}

#[test]
fn package_fmir_image_cli_rest_text_operand_uses_runtime_arguments() {
    let dir = test_temp_dir("package-fmir-cli-rest-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand rest string files
main args args {
  let list<string> files ← args.files
  print files.length()
}
"#,
    )
    .expect("write entry");

    let image = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    )
    .expect("build fmir image");
    let image_bytes = fs::read(&image.image_path).expect("read image");
    assert!(
        !bytes_contain(&image_bytes, b"alpha"),
        "image must not bake runtime rest argv"
    );
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::with_argumenta(vec!["alpha".to_owned(), "beta".to_owned()]);
    run_fmir_image_for_faber(&image, &mut host).expect("run rest CLI fmir image");

    assert_eq!(host.stdout_lines, vec!["2".to_owned()]);
}

#[test]
fn package_fmir_built_image_cli_rest_text_operand_executes() {
    let dir = test_temp_dir("package-fmir-built-cli-rest-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand rest string files
main args args {
  let list<string> files ← args.files
  print files.length()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec![
        "alpha".to_owned(),
        "beta".to_owned(),
        "gamma".to_owned(),
    ]);
    run_fmir_built_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        radix::program::mir::DeviceSelection::Auto,
        &mut host,
    )
    .expect("run source-built rest CLI fmir image");

    assert_eq!(host.stdout_lines, vec!["3".to_owned()]);
}

#[test]
fn package_fmir_text_image_preserves_fixed_cli_exit_code() {
    let dir = test_temp_dir("package-fmir-text-fixed-exit");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
main args args exit 7 {
  print "done"
}
"#,
    )
    .expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");
    assert!(
        image_text.contains("exit_code = 7"),
        "image must serialize fixed exit code:\n{image_text}"
    );
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = ExitRecordingHost::default();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_fmir_text_image_for_faber(&image, &mut host).expect("run fmir-text image");
    }))
    .expect_err("fixed CLI exit should call host.exit");

    assert_eq!(
        panic.downcast_ref::<ExitPanic>().map(|panic| panic.0),
        Some(7)
    );
    assert_eq!(host.buffer.stdout_lines, vec!["done".to_owned()]);
}

#[test]
fn package_fmir_image_preserves_fixed_cli_exit_code() {
    let dir = test_temp_dir("package-fmir-fixed-exit");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
main args args exit 7 {
  print "done"
}
"#,
    )
    .expect("write entry");

    let image = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    )
    .expect("build fmir image");
    let bytes = fs::read(&image.image_path).expect("read image");
    let summary = fmir_image_test_summary(&bytes, &image.image_path).expect("summarize fmir image");
    assert_eq!(summary.exit_code, Some(7));
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = ExitRecordingHost::default();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_fmir_image_for_faber(&image, &mut host).expect("run fmir image");
    }))
    .expect_err("fixed CLI exit should call host.exit");

    assert_eq!(
        panic.downcast_ref::<ExitPanic>().map(|panic| panic.0),
        Some(7)
    );
    assert_eq!(host.buffer.stdout_lines, vec!["done".to_owned()]);
}

#[test]
fn package_fmir_image_matches_text_image_core_facts() {
    let dir = test_temp_dir("package-fmir-binary-text-parity");
    let entry = dir.join("main.fab");
    let module = dir.join("message.fab");
    fs::write(
        &entry,
        r#"
import from "./message" message

@ cli "tool"
@ operand string name
main args args {
  print message.text(args.name)
}
"#,
    )
    .expect("write entry");
    fs::write(
        &module,
        r#"
@ public
fn text(string name) → string {
  return "Salve, §!"(name)
}
"#,
    )
    .expect("write module");

    let config = Config::default().with_target(Target::MirFmir);
    let text_image =
        build_fmir_text_image_for_faber(&config, &entry, &[]).expect("build fmir-text image");
    let binary_image = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    )
    .expect("build fmir image");
    let text = fs::read_to_string(&text_image.image_path).expect("read text image");
    let bytes = fs::read(&binary_image.image_path).expect("read binary image");

    let text_summary =
        fmir_text_image_test_summary(&text, &text_image.image_path).expect("summarize text image");
    let binary_summary =
        fmir_image_test_summary(&bytes, &binary_image.image_path).expect("summarize binary image");

    assert_eq!(text_summary, binary_summary);
    assert_eq!(text_summary.toolchain_version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn package_fmir_image_rejects_bad_version_without_source_fallback() {
    let dir = test_temp_dir("package-fmir-bad-version");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"must not run\" }").expect("write entry");

    let image = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    )
    .expect("build fmir image");
    let mut image_bytes = fs::read(&image.image_path).expect("read image");
    rewrite_binary_image_version(&mut image_bytes, 99);
    fs::write(&image.image_path, image_bytes).expect("write corrupt image");
    fs::remove_file(&entry).expect("remove source after corrupting image");

    let mut host = BufferHost::default();
    let diagnostics = run_fmir_image_for_faber(&image, &mut host)
        .expect_err("bad fmir version should fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_image_version_unsupported")
            && diagnostic_has_arg(diag, "actual", "99")
            && diagnostic_has_arg(diag, "expected", "8")
    }));
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn package_mir_cli_root_text_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["Ian".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected CLI operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["Ian".to_owned()]);
}

#[test]
fn package_mir_manifest_cli_root_text_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-manifest-cli-operand");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "manifest-cli-operand"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("main.fab"),
        r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["Ian".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

    assert!(
        result.is_ok(),
        "expected manifest CLI operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["Ian".to_owned()]);
}

#[test]
fn package_mir_callback_receives_validated_norma_program() {
    let dir = test_temp_dir("package-mir-callback");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs
import from "norma:toml" toml

main {
  fs.write_file("/tmp/faber-package-mir-callback.txt", "salve")
}
"#,
    )
    .expect("write entry");

    let reached = with_lowered_package_mir(&Config::default(), &entry, |lowered| {
        assert_ne!(
            lowered.validated.program().functions,
            [] as [radix::module::mir::MirFunction; 0]
        );
        assert!(
            lowered
                .validated
                .validation()
                .interner
                .as_interner()
                .is_some()
        );
        true
    })
    .expect("package MIR callback should receive linked Norma program");

    assert!(reached);
}

#[cfg(feature = "mir-llvm")]
#[test]
#[ignore = "assembles the emitted package IR with an external llvm-as; manual verifier proof"]
fn package_mir_json_corpus_emits_verifier_valid_llvm() {
    assert_package_corpus_llvm_smoke("json/json.fab", "package-json-corpus");
}

#[cfg(feature = "mir-llvm")]
#[test]
#[ignore = "assembles the emitted package IR with an external llvm-as; manual verifier proof"]
fn package_mir_stage4b_instans_emits_verifier_valid_llvm() {
    assert_package_corpus_llvm_smoke("instans/instans.fab", "package-stage4b-instans");
}

#[cfg(feature = "mir-llvm")]
fn assert_package_corpus_llvm_smoke(relative: &str, label: &str) {
    let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative);
    let emitted = with_lowered_package_mir(&Config::default(), &entry, |lowered| {
        let interner = lowered
            .validated
            .validation()
            .interner
            .as_interner()
            .expect("package MIR carries interner");
        radix::module::mir::emit_llvm_text_probe(radix::module::mir::LlvmEmitRequest::new(
            &lowered.validated,
            interner,
        ))
    })
    .expect("corpus package lowers to MIR")
    .expect("corpus package emits LLVM");

    let dir = test_temp_dir(label);
    let llvm_path = dir.join("module.ll");
    fs::write(&llvm_path, emitted).expect("write package corpus LLVM");
    let output = Command::new("llvm-as")
        .arg(&llvm_path)
        .arg("-o")
        .arg(llvm_path.with_extension("bc"))
        .output()
        .expect("run llvm-as");
    assert!(
        output.status.success(),
        "llvm-as rejected package corpus {relative}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn package_mir_bridges_norma_fs_crea_iunge_exstat_amputa() {
    let dir = test_temp_dir("package-mir-solum-create_dir-delete");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
  mut list<string> partes ← [args.root, "nested"]
  let string nested ← fs.join(partes)
  fs.create_dir(nested)
  print fs.exists(nested)
  fs.remove_tree(nested)
  print fs.exists(nested)
}
"#,
    )
    .expect("write entry");
    let fixture_root = dir.join("workspace");

    let mut host = BufferHost::with_argumenta(vec![fixture_root.to_string_lossy().into_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected fs create_dir/join/exists/remove_tree bridge success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec!["true".to_owned(), "false".to_owned()]
    );
}

#[test]
fn package_mir_bridges_norma_fs_tange_scribe_lege() {
    let dir = test_temp_dir("package-mir-solum-touch-write_file");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
  mut list<string> partes ← [args.root]
  let string dir ← fs.join(partes)
  fs.create_dir(dir)
  partes ← [dir, "target.txt"]
  let string target ← fs.join(partes)
  fs.touch(target)
  fs.write_file(target, "alpha")
  print fs.read_file<string>(target)
  fs.delete(target)
  fs.remove_tree(dir)
}
"#,
    )
    .expect("write entry");
    let fixture_root = dir.join("workspace");

    let mut host = BufferHost::with_argumenta(vec![fixture_root.to_string_lossy().into_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected fs touch/write_file/read_file bridge success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), format!("{diag:?}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["alpha".to_owned()]);
}

#[test]
fn package_mir_norma_fs_read_file_generic_failable_in_main_runs() {
    let dir = test_temp_dir("package-mir-solum-read-file-generic-failable-main");
    let entry = dir.join("main.fab");
    let payload = dir.join("payload.txt");
    fs::write(&payload, "alpha").expect("write payload");
    let payload_root = payload.to_string_lossy().into_owned();
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
  print fs.read_file<string>(args.root)
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec![payload_root]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected generic failable read_file in main to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), format!("{diag:?}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["alpha".to_owned()]);
}

#[test]
fn package_mir_norma_deque_runs() {
    let dir = test_temp_dir("package-mir-norma-deque");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:deque" deque

main {
  mut deque.Deque<int> d ← deque.empty<int>()
  d.append(1)
  print d.length()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected norma:deque generic genus methods to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), format!("{diag:?}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["1".to_owned()]);
}

#[test]
fn package_mir_norma_deque_reads() {
    let dir = test_temp_dir("package-mir-norma-deque-reads");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:deque" deque

main {
  mut deque.Deque<int> d ← deque.from_list<int>([1, 2])
  d.prepend(0)
  print d.length()
  print d.is_empty()
  print d.first()
  print d.last()
  mut deque.Deque<int> e ← deque.empty<int>()
  print e.is_empty()
  print e.first()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected norma:deque reads to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), format!("{diag:?}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec![
            "3".to_owned(),
            "false".to_owned(),
            "0".to_owned(),
            "2".to_owned(),
            "true".to_owned(),
            "none".to_owned(),
        ]
    );
}

#[test]
fn package_mir_norma_deque_pops_no_rebalance() {
    let dir = test_temp_dir("package-mir-norma-deque-pops-no-rebalance");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:deque" deque

main {
  mut deque.Deque<int> d ← deque.from_list<int>([1, 2])
  d.prepend(0)
  d.append(3)
  print d.pop_front()
  print d.pop_back()
  print d.length()
  mut deque.Deque<int> e ← deque.empty<int>()
  print e.pop_front()
  print e.pop_back()
  print e.length()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected norma:deque pops without rebalance to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), format!("{diag:?}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec![
            "0".to_owned(),
            "3".to_owned(),
            "2".to_owned(),
            "none".to_owned(),
            "none".to_owned(),
            "0".to_owned(),
        ]
    );
}

#[test]
fn package_mir_norma_deque_pops() {
    let dir = test_temp_dir("package-mir-norma-deque-pops");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:deque" deque

main {
  mut deque.Deque<int> d ← deque.from_list<int>([1, 2])
  print d.pop_front()
  print d.pop_back()
  print d.length()
  mut deque.Deque<int> e ← deque.empty<int>()
  print e.pop_front()
  print e.pop_back()
  print e.length()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected norma:deque pops with rebalance to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), format!("{diag:?}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec![
            "1".to_owned(),
            "2".to_owned(),
            "0".to_owned(),
            "none".to_owned(),
            "none".to_owned(),
            "0".to_owned(),
        ]
    );
}

#[test]
fn package_mir_norma_deque_to_list() {
    let dir = test_temp_dir("package-mir-norma-deque-to-list");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:deque" deque

main {
  mut deque.Deque<int> d ← deque.from_list<int>([1, 2])
  d.prepend(0)
  print d.to_list().length()
  mut list<int> out ← d.to_list()
  print out.first()
  print out.last()
  mut deque.Deque<int> e ← deque.empty<int>()
  print e.to_list().length()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected norma:deque to_list copy-out to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), format!("{diag:?}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec![
            "3".to_owned(),
            "0".to_owned(),
            "2".to_owned(),
            "0".to_owned(),
        ]
    );
}

#[test]
fn package_mir_norma_deque_basic_ops() {
    let dir = test_temp_dir("package-mir-norma-deque-basic-ops");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:deque" deque

main {
  mut deque.Deque<int> d ← deque.from_list<int>([1, 2])
  d.prepend(0)
  print d.length()
  print d.to_list().length()
  print d.is_empty()
  print d.first()
  print d.last()
  print d.pop_front()
  print d.pop_back()
  print d.length()
  mut deque.Deque<int> e ← deque.empty<int>()
  print e.first()
  print e.pop_front()
  print e.is_empty()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected norma:deque basic ops composed on one deque to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), format!("{diag:?}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec![
            "3".to_owned(),
            "3".to_owned(),
            "false".to_owned(),
            "0".to_owned(),
            "2".to_owned(),
            "0".to_owned(),
            "2".to_owned(),
            "1".to_owned(),
            "none".to_owned(),
            "none".to_owned(),
            "true".to_owned(),
        ]
    );
}

#[test]
fn package_mir_bridges_norma_fs_exscribe_renomina() {
    let dir = test_temp_dir("package-mir-solum-copy-rename");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
  mut list<string> partes ← [args.root]
  let string dir ← fs.join(partes)
  fs.create_dir(dir)
  partes ← [dir, "source.txt"]
  let string source ← fs.join(partes)
  partes ← [dir, "copy.txt"]
  let string copy ← fs.join(partes)
  partes ← [dir, "moved.txt"]
  let string moved ← fs.join(partes)
  fs.touch(source)
  fs.write_file(source, "alpha")
  fs.copy(source, copy)
  fs.rename(copy, moved)
  print fs.read_file<string>(moved)
  fs.delete(source)
  fs.delete(moved)
  fs.remove_tree(dir)
}
"#,
    )
    .expect("write entry");
    let fixture_root = dir.join("workspace");

    let mut host = BufferHost::with_argumenta(vec![fixture_root.to_string_lossy().into_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected fs copy/rename bridge success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["alpha".to_owned()]);
}

#[test]
fn package_mir_bridges_norma_fs_mensura_regularene_directoriumne() {
    let dir = test_temp_dir("package-mir-solum-byte_length");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
  mut list<string> partes ← [args.root, "nested"]
  let string nested ← fs.join(partes)
  fs.create_dir(nested)
  partes ← [nested, "source.bin"]
  let string source ← fs.join(partes)
  fs.touch(source)
  print fs.byte_length(source)
  print fs.is_regular(source)
  print fs.is_directory(nested)
  fs.delete(source)
  fs.remove_tree(nested)
}
"#,
    )
    .expect("write entry");
    let fixture_root = dir.join("workspace");

    let mut host = BufferHost::with_argumenta(vec![fixture_root.to_string_lossy().into_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected fs byte_length/is_regular/is_directory bridge success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec!["0".to_owned(), "true".to_owned(), "true".to_owned()]
    );
    assert!(!fixture_root.join("nested").exists());
}

#[test]
fn package_mir_bridges_norma_fs_modus_mode_verbs() {
    let dir = test_temp_dir("package-mir-solum-mode");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
  mut list<string> partes ← [args.root, "nested"]
  let string nested ← fs.join(partes)
  fs.create_dir(nested)
  partes ← [nested, "target.bin"]
  let string target ← fs.join(partes)
  fs.touch(target)
  fs.set_mode(target, 384)
  print fs.mode(target)
  fs.delete(target)
  fs.remove_tree(nested)
}
"#,
    )
    .expect("write entry");
    let fixture_root = dir.join("workspace");

    let mut host = BufferHost::with_argumenta(vec![fixture_root.to_string_lossy().into_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected fs mode bridge success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["384".to_owned()]);
}

#[test]
fn package_mir_bridges_norma_fs_link_verbs() {
    let dir = test_temp_dir("package-mir-solum-link");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
  mut list<string> partes ← [args.root, "nested"]
  let string nested ← fs.join(partes)
  fs.create_dir(nested)
  partes ← [nested, "source.bin"]
  let string source ← fs.join(partes)
  partes ← [nested, "source-link"]
  let string link ← fs.join(partes)
  let list<b8> payload ← |41 42|
  fs.write_bytes(source, payload)
  fs.create_link(source, link)
  print fs.is_symlink(link)
  print fs.read_link(link)
  fs.delete(link)
  fs.delete(source)
  fs.remove_tree(nested)
}
"#,
    )
    .expect("write entry");
    let fixture_root = dir.join("workspace");
    let source = fixture_root.join("nested").join("source.bin");

    let mut host = BufferHost::with_argumenta(vec![fixture_root.to_string_lossy().into_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected fs link bridge success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec!["true".to_owned(), source.to_string_lossy().into_owned(),]
    );
    assert!(!fixture_root.join("nested").exists());
}

#[test]
fn package_mir_bridges_norma_fs_funde_octeti() {
    let dir = test_temp_dir("package-mir-solum-write_bytes");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
  mut list<string> partes ← [args.root, "dir"]
  let string dir ← fs.join(partes)
  fs.create_dir(dir)
  partes ← [dir, "data.bin"]
  let string target ← fs.join(partes)
  let list<b8> payload ← |41 42 43|
  fs.write_bytes(target, payload)
  print fs.byte_length(target)
  fs.delete(target)
  fs.remove_tree(dir)
}
"#,
    )
    .expect("write entry");
    let fixture_root = dir.join("workspace");

    let mut host = BufferHost::with_argumenta(vec![fixture_root.to_string_lossy().into_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected fs write_bytes/octeti bridge success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["3".to_owned()]);
    assert!(!fixture_root.join("dir").exists());
}

#[test]
fn package_mir_cli_root_text_operand_preserves_literal_backslashes() {
    let dir = test_temp_dir("package-mir-cli-operand-escape");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec![r"line\nraw".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected CLI operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec![r"line\nraw".to_owned()]);
}

#[test]
fn package_mir_cli_root_numerus_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-numerus-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand int count
main args args {
  print args.count
}
"#,
    )
    .expect("write entry");

    for value in ["7", "-7"] {
        let mut host = BufferHost::with_argumenta(vec![value.to_owned()]);
        let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

        assert!(
            result.is_ok(),
            "expected CLI numerus operand package MIR success for {value}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![value.to_owned()]);
    }
}

#[test]
fn package_mir_cli_root_numerus_operand_rejects_non_numeric() {
    let dir = test_temp_dir("package-mir-cli-numerus-non-numeric");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand int count
main args args {
  print args.count
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["not-a-number".to_owned()]);
    let diagnostics = run_mir_for_faber(&Config::default(), &entry, &mut host)
        .expect_err("non-numeric CLI operand should fail");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics
            .iter()
            .any(|diag| { diagnostic_has_issue(diag, "package_mir_cli_surface_unsupported") }),
        "expected CLI argument parsing diagnostic for non-numeric input, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_cli_root_rest_text_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-rest-text-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand string input
@ operand rest string files
main args args {
  print args.input
  let list<string> files ← args.files
  print files.length()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec![
        "seed".to_owned(),
        "alpha".to_owned(),
        "beta".to_owned(),
    ]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected CLI rest text operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["seed".to_owned(), "2".to_owned()]);
}

#[test]
fn package_mir_cli_root_lista_numerus_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-lista-numerus-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand list<int> counts
main args args {
  let list<int> counts ← args.counts
  print counts.length()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["3".to_owned(), "5".to_owned(), "8".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected CLI lista numerus operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["3".to_owned()]);
}

#[test]
fn package_mir_cli_root_octeti_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-octeti-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand list<b8> raw
main args args {
  let list<b8> raw ← args.raw
  print raw.length()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["Az".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected CLI octeti operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["2".to_owned()]);
}

#[test]
fn package_mir_cli_root_flag_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-flag-option");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ option verbose short "v" long "verbose" type bool
main args args {
  print args.verbose
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [
        (vec!["--verbose".to_owned()], "true"),
        (vec!["--verbose=false".to_owned()], "true"),
        (vec!["-v".to_owned()], "true"),
        (Vec::new(), "false"),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

        assert!(
            result.is_ok(),
            "expected CLI flag option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_root_defaulted_text_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-defaulted-text-option");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ option name short "n" long "name" type string coalesce "worker"
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [
        (Vec::new(), "worker"),
        (vec!["--name".to_owned(), "Ian".to_owned()], "Ian"),
        (vec!["--name=Alba".to_owned()], "Alba"),
        (vec!["-n".to_owned(), "Nia".to_owned()], "Nia"),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

        assert!(
            result.is_ok(),
            "expected CLI defaulted text option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_root_optional_text_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-optional-text-option");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ option name long "name" type string
main args args {
  print args.name coalesce "worker"
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [
        (Vec::new(), "worker"),
        (vec!["--name=Ian".to_owned()], "Ian"),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

        assert!(
            result.is_ok(),
            "expected CLI optional text option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_root_defaulted_numerus_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-defaulted-numerus-option");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ option count long "count" type int coalesce 2
main args args {
  print args.count
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [
        (Vec::new(), "2"),
        (vec!["--count=3".to_owned()], "3"),
        (vec!["--count".to_owned(), "-7".to_owned()], "-7"),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

        assert!(
            result.is_ok(),
            "expected CLI defaulted numerus option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_root_global_flag_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-global-flag-option");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ option verbose long "verbose" type bool global
main args args {
  print args.verbose
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [
        (Vec::new(), "false"),
        (vec!["--verbose".to_owned()], "true"),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

        assert!(
            result.is_ok(),
            "expected CLI global flag option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_root_defaulted_numerus_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-defaulted-numerus-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand int count coalesce 9
main args args {
  print args.count
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [(Vec::new(), "9"), (vec!["7".to_owned()], "7")] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

        assert!(
            result.is_ok(),
            "expected CLI defaulted numerus operand package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_rejects_unparsed_arguments_for_root_entry() {
    let dir = test_temp_dir("package-mir-cli-extra-arg");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
main args args {
  print "cli root"
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["unexpected".to_owned()]);
    let diagnostics = run_mir_for_faber(&Config::default(), &entry, &mut host)
        .expect_err("extra CLI arguments should not be ignored");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "package_mir_cli_surface_unsupported")
                && diagnostic_has_arg(diag, "surface", "CLI argument parsing")
        }),
        "expected CLI argument parsing diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_cli_rejects_missing_required_operand() {
    let dir = test_temp_dir("package-mir-cli-missing-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let diagnostics = run_mir_for_faber(&Config::default(), &entry, &mut host)
        .expect_err("missing required CLI operand should fail");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics
            .iter()
            .any(|diag| { diagnostic_has_issue(diag, "package_mir_cli_surface_unsupported") }),
        "expected CLI argument diagnostic for missing operand, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_cli_root_fixed_exit_runs_through_host_exit() {
    let dir = test_temp_dir("package-mir-cli-fixed-exit");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
main args args exit 7 {
  print "done"
}
"#,
    )
    .expect("write entry");

    let mut host = ExitRecordingHost::default();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_mir_for_faber(&Config::default(), &entry, &mut host);
    }))
    .expect_err("fixed CLI exit should call host.exit");

    assert_eq!(
        panic.downcast_ref::<ExitPanic>().map(|panic| panic.0),
        Some(7)
    );
    assert_eq!(host.buffer.stdout_lines, vec!["done".to_owned()]);
}

#[test]
fn package_mir_cli_dynamic_exit_field_runs_through_host_exit() {
    let dir = test_temp_dir("package-mir-cli-dynamic-exit");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operand int code
main args args exit args.code {
  print "done"
}
"#,
    )
    .expect("write entry");

    let mut host = ExitRecordingHost::with_argumenta(vec!["7".to_owned()]);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_mir_for_faber(&Config::default(), &entry, &mut host);
    }))
    .expect_err("dynamic CLI exit should call host.exit");

    assert_eq!(
        panic.downcast_ref::<ExitPanic>().map(|panic| panic.0),
        Some(7)
    );
    assert_eq!(host.buffer.stdout_lines, vec!["done".to_owned()]);
}

#[test]
fn package_mir_cli_rejects_unsupported_option_default() {
    let dir = test_temp_dir("package-mir-cli-unsupported-option-default");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ option count long "count" type int coalesce "bad"
main args args {
  print args.count
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let diagnostics = run_mir_for_faber(&Config::default(), &entry, &mut host)
        .expect_err("type-incompatible CLI option defaults should remain unsupported");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "package_mir_cli_surface_unsupported")
                && diagnostic_has_arg(
                    diag,
                    "surface",
                    "CLI options beyond root boolean flags and scalar values",
                )
        }),
        "expected unsupported option default diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_cli_mounted_command_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
fn run() args args {
  print "running"
}
"#,
    )
    .expect("write jobs");

    let mut host = BufferHost::with_argumenta(vec!["jobs".to_owned(), "run".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

    assert!(
        result.is_ok(),
        "expected mounted CLI package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["running".to_owned()]);
}

#[test]
fn package_mir_cli_mounted_text_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-operand");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
@ operand string name
fn run() args args {
  print args.name
}
"#,
    )
    .expect("write jobs");

    let mut host =
        BufferHost::with_argumenta(vec!["jobs".to_owned(), "run".to_owned(), "Ian".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

    assert!(
        result.is_ok(),
        "expected mounted CLI operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["Ian".to_owned()]);
}

#[test]
fn package_mir_cli_mounted_numerus_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-numerus-operand");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
@ operand int count
fn run() args args {
  print args.count
}
"#,
    )
    .expect("write jobs");

    let mut host =
        BufferHost::with_argumenta(vec!["jobs".to_owned(), "run".to_owned(), "7".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

    assert!(
        result.is_ok(),
        "expected mounted CLI numerus operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["7".to_owned()]);
}

#[test]
fn package_mir_cli_mounted_rest_numerus_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-rest-numerus-operand");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "sum"
@ operand rest int counts
fn sum() args args {
  let list<int> counts ← args.counts
  print counts.length()
}
"#,
    )
    .expect("write jobs");

    let mut host = BufferHost::with_argumenta(vec![
        "jobs".to_owned(),
        "sum".to_owned(),
        "3".to_owned(),
        "5".to_owned(),
        "8".to_owned(),
    ]);
    let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

    assert!(
        result.is_ok(),
        "expected mounted CLI rest numerus operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["3".to_owned()]);
}

#[test]
fn package_mir_cli_mounted_defaulted_text_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-defaulted-operand");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
@ operand string name coalesce "worker"
fn run() args args {
  print args.name
}
"#,
    )
    .expect("write jobs");

    for (argumenta, expected) in [
        (vec!["jobs".to_owned(), "run".to_owned()], "worker"),
        (
            vec!["jobs".to_owned(), "run".to_owned(), "Ian".to_owned()],
            "Ian",
        ),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

        assert!(
            result.is_ok(),
            "expected mounted CLI defaulted operand package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_mounted_flag_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-flag-option");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
@ option verbose short "v" long "verbose" type bool
fn run() args args {
  print args.verbose
}
"#,
    )
    .expect("write jobs");

    for (argumenta, expected) in [
        (vec!["jobs".to_owned(), "run".to_owned()], "false"),
        (
            vec!["jobs".to_owned(), "run".to_owned(), "--verbose".to_owned()],
            "true",
        ),
        (
            vec!["jobs".to_owned(), "run".to_owned(), "-v".to_owned()],
            "true",
        ),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

        assert!(
            result.is_ok(),
            "expected mounted CLI flag option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_mounted_defaulted_text_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-defaulted-option");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
@ option name short "n" long "name" type string coalesce "worker"
fn run() args args {
  print args.name
}
"#,
    )
    .expect("write jobs");

    for (argumenta, expected) in [
        (vec!["jobs".to_owned(), "run".to_owned()], "worker"),
        (
            vec!["jobs".to_owned(), "run".to_owned(), "--name=Ian".to_owned()],
            "Ian",
        ),
        (
            vec![
                "jobs".to_owned(),
                "run".to_owned(),
                "-n".to_owned(),
                "Nia".to_owned(),
            ],
            "Nia",
        ),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

        assert!(
            result.is_ok(),
            "expected mounted CLI defaulted option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_mounted_optional_text_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-optional-option");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
@ option name long "name" type string
fn run() args args {
  print args.name coalesce "worker"
}
"#,
    )
    .expect("write jobs");

    for (argumenta, expected) in [
        (vec!["jobs".to_owned(), "run".to_owned()], "worker"),
        (
            vec!["jobs".to_owned(), "run".to_owned(), "--name=Ian".to_owned()],
            "Ian",
        ),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

        assert!(
            result.is_ok(),
            "expected mounted CLI optional option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_mounted_global_flag_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-global-flag-option");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ option verbose long "verbose" type bool global
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
fn run() args args {
  print args.verbose
}
"#,
    )
    .expect("write jobs");

    for (argumenta, expected) in [
        (vec!["jobs".to_owned(), "run".to_owned()], "false"),
        (
            vec!["--verbose".to_owned(), "jobs".to_owned(), "run".to_owned()],
            "true",
        ),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

        assert!(
            result.is_ok(),
            "expected mounted CLI global flag option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_mounted_global_defaulted_text_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-global-defaulted-option");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ option name long "name" type string coalesce "worker" global
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
fn run() args args {
  print args.name
}
"#,
    )
    .expect("write jobs");

    for (argumenta, expected) in [
        (vec!["jobs".to_owned(), "run".to_owned()], "worker"),
        (
            vec!["--name=Ian".to_owned(), "jobs".to_owned(), "run".to_owned()],
            "Ian",
        ),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

        assert!(
            result.is_ok(),
            "expected mounted CLI global defaulted option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_mounted_global_optional_text_option_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-global-optional-option");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ option name long "name" type string global
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
fn run() args args {
  print args.name coalesce "worker"
}
"#,
    )
    .expect("write jobs");

    for (argumenta, expected) in [
        (vec!["jobs".to_owned(), "run".to_owned()], "worker"),
        (
            vec!["--name=Ian".to_owned(), "jobs".to_owned(), "run".to_owned()],
            "Ian",
        ),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

        assert!(
            result.is_ok(),
            "expected mounted CLI global optional option package MIR success for {argumenta:?}, got {:?}",
            result
                .err()
                .unwrap_or_default()
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        assert_eq!(host.stdout_lines, vec![expected.to_owned()]);
    }
}

#[test]
fn package_mir_cli_mounted_global_text_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-global-operand");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ operand string tenant global
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
@ operand string name
fn run() args args {
  print "§:§"(args.tenant, args.name)
}
"#,
    )
    .expect("write jobs");

    let mut host = BufferHost::with_argumenta(vec![
        "jobs".to_owned(),
        "run".to_owned(),
        "acme".to_owned(),
        "Ian".to_owned(),
    ]);
    let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

    assert!(
        result.is_ok(),
        "expected mounted CLI global operand package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["acme:Ian".to_owned()]);
}

#[test]
fn package_mir_cli_mounted_alias_text_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-mounted-alias");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
@ alias "start"
@ operand string name
fn run() args args {
  print args.name
}
"#,
    )
    .expect("write jobs");

    let mut host = BufferHost::with_argumenta(vec![
        "jobs".to_owned(),
        "start".to_owned(),
        "Ian".to_owned(),
    ]);
    let result = run_mir_for_faber(&Config::default(), &dir, &mut host);

    assert!(
        result.is_ok(),
        "expected mounted CLI alias package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["Ian".to_owned()]);
}

#[test]
fn package_mir_linking_executes_text_returning_local_import() {
    let dir = test_temp_dir("package-mir-linking-text-remap");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./thing" thing

main {
  print thing.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r#"
@ public
fn label() → string {
  return "ok"
}
"#,
    )
    .expect("write module");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected text-returning local import to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["ok".to_owned()]);
}

#[test]
fn package_mir_manifest_imports_shared_text_helper_outside_source_root() {
    let fixture = coreutils_like_package("package-mir-shared-outside-source");
    fs::write(
        fixture.src.join("main.fab"),
        r#"
import from "§gnu/format" gnu_format

@ cli "echo"
@ operand string word
main args args {
  print gnu_format.identity(args.word)
}
"#,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("format.fab"),
        r"
@ public
fn identity(string word) → string {
  return word
}
",
    )
    .expect("write shared helper");

    let mut host = BufferHost::with_argumenta(vec!["salve".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &fixture.package, &mut host);

    assert!(
        result.is_ok(),
        "expected shared helper outside source root to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["salve".to_owned()]);
}

#[test]
fn package_mir_manifest_shared_rest_operand_joiner_executes() {
    let fixture = coreutils_like_package("package-mir-shared-rest-joiner");
    fs::write(
        fixture.src.join("main.fab"),
        r#"
import from "§gnu/format" gnu_format

@ cli "echo"
@ operand rest string words
main args args {
  print gnu_format.inter_spatia(args.words)
}
"#,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("format.fab"),
        r#"
@ public
fn inter_spatia(list<string> verba) → string {
  if verba.length() ≡ 0 then return ""
  mut string linea ← verba[0]
  mut int i ← 1
  while i ≺ verba.length() {
    linea ← linea + " " + verba[i]
    i ← i + 1
  }
  return linea
}
"#,
    )
    .expect("write shared formatter");

    let mut host = BufferHost::with_argumenta(vec!["salve".to_owned(), "munde".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &fixture.package, &mut host);

    assert!(
        result.is_ok(),
        "expected shared rest-operand formatter to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["salve munde".to_owned()]);
}

#[test]
fn package_mir_manifest_shared_import_chain_outside_source_root_executes() {
    let fixture = coreutils_like_package("package-mir-shared-import-chain");
    fs::write(
        fixture.src.join("main.fab"),
        r#"
import from "§gnu/stdio" gnu_stdio

@ cli "echo"
@ operand string word
main args args {
  gnu_stdio.scribe_identity(args.word)
}
"#,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("stdio.fab"),
        r#"
import from "./format" format

@ public
fn scribe_identity(string word) → void {
  print format.identity(word)
}
"#,
    )
    .expect("write shared stdio");
    fs::write(
        fixture.common.join("format.fab"),
        r"
@ public
fn identity(string word) → string {
  return word
}
",
    )
    .expect("write shared format");

    let mut host = BufferHost::with_argumenta(vec!["salve".to_owned()]);
    let result = run_mir_for_faber(&Config::default(), &fixture.package, &mut host);

    assert!(
        result.is_ok(),
        "expected shared import chain to run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["salve".to_owned()]);
}

#[test]
fn package_mir_manifest_shared_import_cycle_reports_cycle() {
    let fixture = coreutils_like_package("package-mir-shared-import-cycle");
    fs::write(
        fixture.src.join("main.fab"),
        r#"
import from "§gnu/a" a

main {
  print a.value()
}
"#,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("a.fab"),
        r#"
import from "./b" b

fn value() → int {
  return b.value()
}
"#,
    )
    .expect("write shared a");
    fs::write(
        fixture.common.join("b.fab"),
        r#"
import from "./a" a

fn value() → int {
  return a.value()
}
"#,
    )
    .expect("write shared b");

    let mut host = BufferHost::default();
    let diagnostics = run_mir_for_faber(&Config::default(), &fixture.package, &mut host)
        .expect_err("shared import cycle should fail package analysis");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "package_import_cycle")),
        "expected shared import cycle diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_manifest_shared_private_export_reports_namespace_diagnostic() {
    let fixture = coreutils_like_package("package-mir-shared-private-export");
    fs::write(
        fixture.src.join("main.fab"),
        r#"
import from "§gnu/format" gnu_format

main {
  print gnu_format.hidden("salve")
}
"#,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("format.fab"),
        r"
@ private
fn hidden(string word) → string {
  return word
}

fn identity(string word) → string {
  return word
}
",
    )
    .expect("write shared format");

    let mut host = BufferHost::default();
    let diagnostics = run_mir_for_faber(&Config::default(), &fixture.package, &mut host)
        .expect_err("private shared export should fail package analysis");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "import_module_private")
                && diagnostic_has_arg(diag, "name", "hidden")
        }),
        "expected private shared export diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_linking_executes_non_entry_local_call_chain() {
    let dir = test_temp_dir("package-mir-linking-non-entry-chain");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./service" Service

main {
  Service.run()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("service.fab"),
        r#"
import from "./util" U

@ public
fn run() → void {
  print U.adde(2, 3)
}
"#,
    )
    .expect("write service");
    fs::write(
        dir.join("util.fab"),
        r"
@ public
fn adde(int left, int right) → int {
  return left + right
}
",
    )
    .expect("write util");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected package MIR run success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["5".to_owned()]);
}

#[test]
fn package_mir_linking_keeps_duplicate_file_stem_functions_path_scoped() {
    let dir = test_temp_dir("package-mir-linking-duplicate-stems");
    fs::create_dir_all(dir.join("a")).expect("create a");
    fs::create_dir_all(dir.join("b")).expect("create b");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./a/util" A
import from "./b/util" B

main {
  print A.label()
  print B.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("a").join("util.fab"),
        r"
@ public
fn label() → int {
  return 1
}
",
    )
    .expect("write a util");
    fs::write(
        dir.join("b").join("util.fab"),
        r"
@ public
fn label() → int {
  return 2
}
",
    )
    .expect("write b util");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected package MIR run success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["1".to_owned(), "2".to_owned()]);
}

#[test]
fn package_mir_linking_reports_unmarked_namespace_member_as_module_private() {
    // D5.1: an unmarked top-level declaration is module-private on the
    // package path too — one export-visibility resolver for both paths.
    let dir = test_temp_dir("package-mir-unmarked-namespace");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./auxilium" Aux

main {
  print Aux.occultum()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r"
fn occultum() → int {
  return 1
}

@ public
fn publicum() → int {
  return 2
}
",
    )
    .expect("write auxilium");

    let mut host = BufferHost::default();
    let diagnostics = run_mir_for_faber(&Config::default(), &entry, &mut host)
        .expect_err("unmarked namespace member access should fail package analysis");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "import_module_private")
                && diagnostic_has_arg(diag, "name", "occultum")
        }),
        "expected module-private diagnostic for unmarked member, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_linking_reports_private_namespace_diagnostic() {
    let dir = test_temp_dir("package-mir-private-namespace");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./auxilium" Aux

main {
  print Aux.secretum()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r"
@ private
fn secretum() → int {
  return 1
}

fn publicum() → int {
  return 2
}
",
    )
    .expect("write auxilium");

    let mut host = BufferHost::default();
    let diagnostics = run_mir_for_faber(&Config::default(), &entry, &mut host)
        .expect_err("private namespace access should fail package analysis");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "import_module_private")
                && diagnostic_has_arg(diag, "name", "secretum")
        }),
        "expected private export diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_linking_reports_nested_namespace_call_as_unsupported() {
    let dir = test_temp_dir("package-mir-nested-namespace");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./auxilium" Aux

main {
  print Aux.mathesis.dupla(4)
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r"
fn dupla(int n) → int {
  return n * 2
}
",
    )
    .expect("write auxilium");

    let mut host = BufferHost::default();
    let diagnostics = run_mir_for_faber(&Config::default(), &entry, &mut host)
        .expect_err("nested namespace calls should be explicitly unsupported");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "namespace_missing_export")
                && diagnostic_has_arg(diag, "member", "mathesis.dupla")
        }),
        "expected nested namespace diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_linking_executes_norma_source_library_import() {
    // LIB-MIR: non-bridged `norma:*` source libraries are linked into the
    // package MIR program and execute. Previously this import failed closed
    // with `package_mir_library_imports_unsupported`.
    let dir = test_temp_dir("package-mir-norma-linked");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:text" text

main {
  print text.reverse("roma")
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "norma source-library import should link and run, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["amor".to_owned()]);
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_field_names_default_preserves_source_spelling() {
    let dir = test_temp_dir("rust-field-names-preserve");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
class PersonaNova {
  mut string nomenLongum
}

main {
  let _ p ← PersonaNova { nomenLongum = "Marcus" }
  print p.nomenLongum
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub nomenLongum: String"));
    assert!(output.code.contains("nomenLongum: \"Marcus\".to_string()"));
    assert!(output.code.contains("p.nomenLongum"));
    assert!(!output.code.contains("nomen_longum"));
}

/// `rust_field_names = "snake_case"` renames the generated Rust identifiers, but
/// a genus and a discretio variant print their construction literal (D2.8,
/// rd4a/pr3), so the source spelling legitimately survives inside the `Debug`
/// `write!` format string. Any other occurrence is an un-renamed identifier.
#[cfg(feature = "hir-rust")]
fn assert_source_field_name_only_in_print_literal(code: &str, source_name: &str) {
    for line in code.lines().filter(|line| line.contains(source_name)) {
        assert!(
            line.contains("write!(f, \""),
            "source field name {source_name:?} survived outside the D2.8 print literal: {line}"
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_field_names_snake_case_manifest_policy_renames_generated_fields() {
    let dir = test_temp_dir("rust-field-names-snake-case");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "field-policy"

[paths]
entry = "main.fab"

[build]
rust_field_names = "snake_case"
"#,
    )
    .expect("write manifest");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    let entry = src.join("main.fab");
    fs::write(
        &entry,
        r#"
class PersonaNova {
  mut string nomenLongum
}

main {
  let _ p ← PersonaNova { nomenLongum = "Marcus" }
  print p.nomenLongum
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub nomen_longum: String"));
    assert!(output.code.contains("nomen_longum: \"Marcus\".to_string()"));
    assert!(output.code.contains("p.nomen_longum"));
    assert!(!output.code.contains("pub nomenLongum"));
    assert!(!output.code.contains("p.nomenLongum"));
    assert_source_field_name_only_in_print_literal(&output.code, "nomenLongum");
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_field_names_snake_case_manifest_policy_rejects_field_collisions() {
    let dir = test_temp_dir("rust-field-names-snake-case-collision");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "field-policy"

[paths]
entry = "main.fab"

[build]
rust_field_names = "snake_case"
"#,
    )
    .expect("write manifest");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    let entry = src.join("main.fab");
    fs::write(
        &entry,
        r#"
class Collidens {
  mut string fooBar
  mut string foo_bar
}

main {
  print "x"
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "rust_field_name_collision")
            && diagnostic_has_arg(diag, "owner_kind", "genus")
            && diagnostic_has_arg(diag, "owner", "Collidens")
            && diagnostic_has_arg(diag, "previous_field", "fooBar")
            && diagnostic_has_arg(diag, "field", "foo_bar")
            && diagnostic_has_arg(diag, "rust_field", "foo_bar")
    }));
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_field_names_snake_case_manifest_policy_renames_variant_fields() {
    let dir = test_temp_dir("rust-field-names-snake-case-variant");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "field-policy"

[paths]
entry = "main.fab"

[build]
rust_field_names = "snake_case"
"#,
    )
    .expect("write manifest");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    let entry = src.join("main.fab");
    fs::write(
        &entry,
        r#"
union Nuntius {
  Clavis { string nomenLongum }
}

fn tracta(Nuntius n) → string {
  match n {
    case Clavis let nomenLongum as captum {
      return captum
    }
  }
}

main {
  let Nuntius n ← variant Clavis { nomenLongum = "Julia" } ∷ Nuntius
  print tracta(n)
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("nomen_longum: String"));
    assert!(output.code.contains("nomen_longum: l_captum"));
    assert_source_field_name_only_in_print_literal(&output.code, "nomenLongum");
}

#[test]
fn rust_field_names_manifest_policy_rejects_unknown_values() {
    let dir = test_temp_dir("rust-field-names-invalid");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "field-policy"

[build]
rust_field_names = "camel"
"#,
    )
    .expect("write manifest");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    let entry = src.join("main.fab");
    fs::write(&entry, "main { print \"x\" }").expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "invalid_package_manifest"))
    );
}

#[test]
fn rust_target_manifest_accepts_native_host_policy() {
    let dir = test_temp_dir("rust-target-native-host-policy");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "native-host-policy"

[target.rust]
host = "native"
"#,
    )
    .expect("write manifest");

    let manifest = read_manifest(&dir.join("faber.toml")).expect("read manifest");
    assert!(
        manifest
            .target
            .get("rust")
            .and_then(|target| target.host)
            .is_some()
    );
}

#[test]
fn non_rust_target_manifest_rejects_host_policy() {
    let dir = test_temp_dir("non-rust-target-host-policy");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "bad-host-policy"

[paths]
entry = "main.fab"

[target.scena]
host = "native"
"#,
    )
    .expect("write manifest");
    let manifest_path = dir.join("faber.toml");
    let manifest = read_manifest(&manifest_path).expect("read manifest");
    let err = validate_manifest(&manifest, &manifest_path).expect_err("host policy must fail");
    assert!(diagnostic_has_issue(&err, "invalid_target_host"));
}

#[test]
fn manifest_device_steps_zero_fails_closed() {
    // S5-U5b: `[device] steps = 0` is a contradiction (a RepeatingStep
    // program must drive at least one step) — the manifest validation rejects
    // it at package load, never silently treated as absent.
    let dir = test_temp_dir("device-steps-zero");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "device-steps-zero"

[paths]
entry = "main.fab"

[device]
steps = 0
"#,
    )
    .expect("write manifest");
    let manifest_path = dir.join("faber.toml");
    let manifest = read_manifest(&manifest_path).expect("read manifest");
    let err = validate_manifest(&manifest, &manifest_path)
        .expect_err("a zero declared step count must fail closed");
    assert!(diagnostic_has_issue(&err, "package_device_steps_zero"));
}

#[test]
fn manifest_device_steps_parses_and_validates() {
    // S5-U5b: a positive `[device] steps` parses into the manifest device
    // surface and validates cleanly at package load.
    let dir = test_temp_dir("device-steps-valid");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "device-steps-valid"

[paths]
entry = "main.fab"

[device]
device = "auto"
steps = 250
"#,
    )
    .expect("write manifest");
    let manifest_path = dir.join("faber.toml");
    let manifest = read_manifest(&manifest_path).expect("read manifest");
    validate_manifest(&manifest, &manifest_path).expect("a positive step count validates");
    assert_eq!(manifest.device.steps, Some(250));
}

// S3-U2 cut: this fail-closed proof exercises the device-section builder,
// which compiles only under `device-runtime` (now opt-in, not in
// `full-targets`). Gated to the device feature; exercised by the opt-in
// runner proof (S3-U4).
#[cfg(feature = "device-runtime")]
#[test]
fn package_fmir_image_rejects_device_steps_on_single_run_program() {
    // S5-U5b fail-closed policy: the `[device] steps` channel applies only
    // to a RepeatingStep training program. A SingleRun device package
    // (compute kernels, no training loop) that declares a step count fails
    // package construction closed — never a silently dropped count.
    let dir = test_temp_dir("package-device-steps-single");
    fs::create_dir_all(dir.join("src")).expect("src dir");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "device-steps-single"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"

[device]
device = "auto"
steps = 250
"#,
    )
    .expect("write manifest");
    fs::write(
        dir.join("src").join("main.fab"),
        r"@ kernel
fn summa(tf32[2] x) → f32 {
    return x.sum()
}
",
    )
    .expect("write entry");
    let entry = dir.join("src").join("main.fab");
    let result = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    );
    let diagnostics = result.expect_err("steps on a SingleRun device program must fail closed");
    assert!(
        diagnostics
            .iter()
            .any(|diag| { diagnostic_has_issue(diag, "package_device_steps_not_repeating") })
    );
}

/// `faber build` of a `[device]` package always emits the Metal artifact, so a
/// kernel Metal's requirement row refuses (a 48-bit intermediate in a `u32`
/// expression) stops the build by name before any emission.
#[cfg(feature = "device-runtime")]
#[test]
fn package_fmir_image_refuses_a_kernel_the_metal_row_refuses() {
    let dir = test_temp_dir("package-device-metal-row");
    fs::create_dir_all(dir.join("src")).expect("src dir");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "device-metal-row"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"

[device]
device = "auto"
"#,
    )
    .expect("write manifest");
    fs::write(
        dir.join("src").join("main.fab"),
        "+++\nlocale = \"en\"\n+++\n@ kernel
@ public
fn k_wide(tensor<u32, [8]> a, tensor<u32, [8]> b, mut tensor<u32, [8]> out) → void {
    for from a at [i] thread lane let x {
        out[[i]] ← ((x ∧ 16777215) * (b[[i]] ∧ 16777215)) / 1048576
    }
}
main {}
",
    )
    .expect("write entry");
    let entry = dir.join("src").join("main.fab");
    let result = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    );
    let diagnostics = result.expect_err("a kernel Metal refuses must not build a device section");
    assert!(
        diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "kernel_target_requirements_not_met")),
        "{diagnostics:?}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_text_native_body() {
    let dir = test_temp_dir("norma-chorda-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:text" text

main {
  let string reversed ← text.reverse("roma")
  print reversed
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:text package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn reverse"));
    assert!(output.code.contains("reverse(\"roma\".to_string())"));
    assert!(output.code.contains("pub mod text"));
    assert!(output.code.contains("crate::text::reverse"));
    assert!(!output.code.contains("norma::text::reverse"));
    assert!(!output.code.contains("panic!(\"unimplemented\")"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_type_only_norma_file_namespace_import() {
    let dir = test_temp_dir("norma-net-endpoint-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:net/endpoint" endpoint

main {
  let endpoint.Endpoint endpoint ← endpoint.Endpoint {
    host = "localhost",
    port = 8080
  }
  print endpoint.host
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:net/endpoint package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "unused_import"))
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub mod net"));
    assert!(output.code.contains("pub mod endpoint"));
    assert!(output.code.contains("pub struct Endpoint"));
    assert!(output.code.contains(
        "let l_endpoint: crate::net::endpoint::Endpoint = crate::net::endpoint::Endpoint"
    ));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_type_only_local_file_namespace_import() {
    let dir = test_temp_dir("local-type-only-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./thing" Alias

main {
  let Alias.Thing item ← Alias.Thing { name = "ok" }
  print item.name
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r"
@ public
class Thing {
    mut string name
}
",
    )
    .expect("write type module");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected local type-only package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "unused_import"))
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub mod thing"));
    assert!(output.code.contains("pub struct Thing"));
    assert!(
        output
            .code
            .contains("let l_item: crate::thing::Thing = crate::thing::Thing")
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_preserves_file_stem_named_type_under_import_alias() {
    let library_home = test_temp_dir("alias-stem-type-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "Thing",
        r#"+++
locale = "la"
+++
@ publica
genus Thing {
    varia textus name
}
"#,
    );

    let dir = test_temp_dir("alias-stem-type-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:Thing" alias

main {
    let alias.Thing item ← alias.Thing { name = "ok" }
    print item.name
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&config_with_library_home(library_home.path()), &entry);
    assert!(
        result.success(),
        "expected aliased file-stem type package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(!output.code.contains("pub mod alias"));
    assert!(output.code.contains("pub struct Thing"));
    assert!(
        output
            .code
            .contains("let l_item: crate::Thing::Thing = crate::Thing::Thing")
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_text_diducta_native_body() {
    let dir = test_temp_dir("norma-chorda-diducta-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:text" text

main {
  do {
    let [meta, corpus] ← text.split_frontmatter("+++\na = 1\n+++\nbody", "+++")
    print meta, corpus
  } catch err {
    warn err
  }
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:text diducta package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn split_frontmatter"));
    assert!(output.code.contains("fn delimiter_end"));
    assert!(!output.code.contains("norma::text::split_frontmatter"));
    assert!(!output.code.contains("faber::frame::sermo_open"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_chorda_retenta_filters_codegen() {
    let dir = test_temp_dir("norma-chorda-retenta-filters");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:text" text

main {
  print text.keep_between("hello7!", "a", "z")
  print text.keep_if("hello7!", string c ∴ c ≥ "a" and c ≤ "z")
  print text.drop_between("hello7!", "0", "9")
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected retenta filter compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(
        output.code.contains("fn keep_between("),
        "interval filter should export retenta: {}",
        output.code
    );
    assert!(
        output.code.contains("fn keep_if("),
        "predicate filter should export retenta_iudicio: {}",
        output.code
    );
    assert!(
        !output.code.contains("Some(|"),
        "predicate overload should not wrap closure in Some: {}",
        output.code
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_tensor_native_body() {
    let dir = test_temp_dir("norma-tensor-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:tensor" tensor

main {
  print "tensor"
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:tensor package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub mod tensor"));
    assert!(output.code.contains("fn flatten"));
    assert!(output.code.contains("fn construct"));
    assert!(!output.code.contains("norma::tensor::flatten"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_omits_nondum_norma_text_functions_from_output() {
    let dir = test_temp_dir("norma-chorda-mixed");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:text" text

main {
  print text.reverse("Salve")
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:text package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn reverse"));
    assert!(output.code.contains("pub mod text"));
    assert!(output.code.contains("crate::text::reverse"));
    // N4a: encode/decode are ordinary Faber bodies and must be emitted.
    assert!(output.code.contains("pub(crate) fn encode"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_emits_norma_text_encode() {
    let dir = test_temp_dir("norma-text-encode");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:text" text

main {
  let list<b8> raw ← text.encode("Salve")
  print text.decode(raw)
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:text encode/decode compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_emits_norma_toml_parse() {
    let dir = test_temp_dir("norma-toml-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:toml" toml

main {
  let _ parsed ← toml.parse("name = \"faber\"")
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:toml parse compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_value_native_navigation_body() {
    let dir = test_temp_dir("norma-value-native-nav");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:value" value

main {
  let value doc ← { "id": "sample" } ↦ value
  do {
    let string id ← value.require_string(doc, "id")
    print id
  } catch err {
    warn err
  }
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:value native navigation compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn get"));
    assert!(output.code.contains("fn require_string"));
    assert!(!output.code.contains("norma::value::get"));
    assert!(!output.code.contains("norma::value::require_string"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_value_exige_claves_native_body() {
    let dir = test_temp_dir("norma-value-exige-claves");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:value" value

main {
  let value doc ← { "id": "sample" } ↦ value
  do {
    value.require_strings(doc, ["id", "name"])
    print "ok"
  } catch err {
    warn err
  }
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:value require_strings compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn require_strings"));
    assert!(!output.code.contains("norma::value::require_strings"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_fs_explora_contract() {
    let dir = test_temp_dir("norma-solum-explora-contract");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs

main {
  print "fs"
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:fs explora contract compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn glob"));
    assert!(!output.code.contains("norma::fs::glob"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_console_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-consolum-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:console" console

main {
  console.write_partial("salve")
  let _ tty ← console.stdin_is_tty()
  console.write_bytes(|48 65 6c 6c 6f|)
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:console package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("consolum:dic"));
    assert!(output.code.contains("consolum:audit"));
    assert!(output.code.contains("faber::frame::sermo_open"));
    assert!(output.code.contains("fn write_partial"));
    assert!(output.code.contains("crate::console::write_bytes"));
    assert!(output.code.contains("pub(crate) fn write_bytes"));
    assert!(!output.code.contains("norma::console::write_bytes"));
    assert!(!output.code.contains("norma::console::write_partial"));
    assert!(!output.code.contains("norma::console::stdin_is_tty"));
    assert!(!output.code.contains("crate::norma::hal::console"));
}

/// Output sinks must not lower through `format!`. Two legitimate sources of
/// `format!(` exist in a package build: the generated `Debug` impl of a genus,
/// which prints its construction literal (D2.8, rd4a/pr3) — imported norma
/// genera such as `scrinium` carry one, with `format!` for optional fields —
/// and the `return Err(format!(...))` a failable string `throw` lowers to in a
/// transitively imported module (`norma:value`'s `require_string`, reached
/// through `norma:fault`, carries one). An output sink returns
/// `Result<(), fault::IoFault>`, so it never produces that `Err` shape; a
/// regression that lowered a sink through `format!` would still fail here.
#[cfg(feature = "hir-rust")]
fn assert_format_macro_only_in_expected_sites(code: &str) {
    for line in code.lines().filter(|line| line.contains("format!(")) {
        assert!(
            line.contains("write!(f, \"") || line.trim_start().starts_with("return Err(format!("),
            "format! outside a D2.8 construction-literal Debug impl or a failable throw: {line}"
        );
    }
    for member in [
        "write_partial",
        "write_bytes",
        "write_line",
        "warn_line",
        "debug_line",
    ] {
        let start = code
            .find(&format!("fn {member}("))
            .unwrap_or_else(|| panic!("expected generated sink fn {member}"));
        let body = &code[start..];
        let end = body.find("\n    }\n").unwrap_or(body.len());
        assert!(
            !body[..end].contains("format!("),
            "output sink {member} lowered through format!:\n{}",
            &body[..end]
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn consolum_eot4_native_output_sinks_canonical() {
    let dir = test_temp_dir("consolum-eot4-canonical");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:console" const write_partial as partial_sink, write_bytes as bytes_sink, write_line as line_sink, warn_line as warning_sink, debug_line as debug_sink
import from "norma:fault" fault

fn drain_sinks() → void ⇥ fault.IoFault {
  partial_sink("partial")
  partial_sink("chain")
  bytes_sink(|41 42|)
  line_sink("line")
  warning_sink("warning")
  debug_sink("debug")
}

main {
  drain_sinks()
}
"#,
    )
    .expect("write canonical consolum fixture");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "canonical consolum output fixture should compile: {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };

    for member in [
        "write_partial",
        "write_bytes",
        "write_line",
        "warn_line",
        "debug_line",
    ] {
        assert!(
            output.code.contains(&format!("crate::console::{member}")),
            "expected in-crate consolum call for {member}:\n{}",
            output.code
        );
    }
    for route in [
        "consolum:dic",
        "consolum:funde",
        "consolum:scribe",
        "consolum:mone",
        "consolum:vide",
    ] {
        assert!(
            output.code.contains(route),
            "expected consolum provider route {route}:\n{}",
            output.code
        );
    }

    assert!(!output.code.contains("println!"));
    assert_format_macro_only_in_expected_sites(&output.code);
}

#[cfg(feature = "hir-rust")]
#[test]
fn consolum_eot4_native_output_sinks_la_surface() {
    let dir = test_temp_dir("consolum-eot4-la");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:console" fixum dic ut partial_sink
importa ex "norma:console" fixum funde ut bytes_sink
importa ex "norma:console" fixum scribe ut line_sink
importa ex "norma:console" fixum mone ut warning_sink
importa ex "norma:console" fixum vide ut debug_sink
importa ex "norma:fault" fault

functio drain_sinks() → vacuum ⇥ fault.IoFault {
  partial_sink("partial")
  partial_sink("chain")
  bytes_sink(|43 44|)
  line_sink("line")
  warning_sink("warning")
  debug_sink("debug")
}

incipit {
  drain_sinks()
}
"#,
    )
    .expect("write la consolum fixture");

    let la_path = radix::module::locale::workspace_locale_pack_path("la");
    let la_pack =
        radix::module::locale::LocalePack::from_toml_path(la_path).expect("load la locale pack");
    let config = Config::default().with_locale_pack(la_pack);
    let result = compile_with_faber_seams(&config, &entry);
    assert!(
        result.success(),
        "la-surface consolum output fixture should compile: {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };

    for surface in ["dic", "funde", "scribe", "mone", "vide"] {
        assert!(
            output.code.contains(&format!("crate::console::{surface}")),
            "expected la-surface member to lower to in-crate consolum call for {surface}:\n{}",
            output.code
        );
    }
    for route in [
        "consolum:dic",
        "consolum:funde",
        "consolum:scribe",
        "consolum:mone",
        "consolum:vide",
    ] {
        assert!(
            output.code.contains(route),
            "expected la-surface consolum provider route {route}:\n{}",
            output.code
        );
    }

    assert!(!output.code.contains("println!"));
    assert_format_macro_only_in_expected_sites(&output.code);
}

#[cfg(feature = "hir-rust")]
#[test]
fn consolum_eot4_input_values_canonical() {
    let dir = test_temp_dir("consolum-eot4-input-canonical");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:console" const read_line as line_reader, read_bytes as bytes_reader

main {
  let string line ← line_reader() coalesce ""
  let list<b8> chunk ← bytes_reader(4) coalesce |00|
}
"#,
    )
    .expect("write canonical consolum input fixture");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "canonical consolum input fixture should compile: {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };

    assert!(
        output.code.contains("crate::console::read_line"),
        "expected in-crate read_line call:\n{}",
        output.code
    );
    assert!(
        output.code.contains("crate::console::read_bytes"),
        "expected in-crate read_bytes call:\n{}",
        output.code
    );
    assert!(output.code.contains("fn read_line"));
    assert!(output.code.contains("pub(crate) fn read_bytes"));
    assert!(output.code.contains("consolum:lege"));
    assert!(output.code.contains("consolum:hauri"));
    assert!(
        output.code.contains("consolum:desinit"),
        "expected console readers to probe consolum:desinit before reading:\n{}",
        output.code
    );
    assert!(output.code.contains("faber::frame::sermo_open"));
    assert!(!output.code.contains("norma::console::read_line"));
    assert!(!output.code.contains("norma::console::read_bytes"));
    assert!(!output.code.contains("crate::norma::hal::console"));
    assert!(!output.code.contains("builtin-claim"));
    assert!(!output.code.contains("builtin-claimed"));
    assert!(!output.code.contains("println!"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn consolum_eot4_input_values_la_surface() {
    let dir = test_temp_dir("consolum-eot4-input-la");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:console" fixum lege ut line_reader
importa ex "norma:console" fixum hauri ut bytes_reader
importa ex "norma:console" fixum scribe ut line_sink
importa ex "norma:console" fixum funde ut bytes_sink
importa ex "norma:fs" fixum lege ut terra_lege

incipit {
  line_sink(line_reader() vel "")
  bytes_sink(bytes_reader(4) vel |00|)
  line_sink(terra_lege("data.txt"))
}
"#,
    )
    .expect("write la consolum input fixture");

    let la_path = radix::module::locale::workspace_locale_pack_path("la");
    let la_pack =
        radix::module::locale::LocalePack::from_toml_path(la_path).expect("load la locale pack");
    let config = Config::default().with_locale_pack(la_pack);
    let result = compile_with_faber_seams(&config, &entry);
    assert!(
        result.success(),
        "la-surface consolum input fixture should compile: {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };

    assert!(
        output
            .code
            .contains("use crate::console::lege as line_reader"),
        "expected la lege to resolve to the console member, not fs:\n{}",
        output.code
    );
    assert!(
        output
            .code
            .contains("use crate::console::hauri as bytes_reader"),
        "expected la hauri to resolve to the consolum member:\n{}",
        output.code
    );
    assert!(
        output.code.contains("use crate::fs::lege as terra_lege"),
        "expected fs lege to stay module-scoped to fs:\n{}",
        output.code
    );
    assert!(output.code.contains("pub(crate) fn read_line"));
    assert!(output.code.contains("pub(crate) fn read_bytes"));
    assert!(
        output.code.contains("crate::fs::lege"),
        "expected fs lege stub to be emitted:\n{}",
        output.code
    );
    assert!(output.code.contains("consolum:lege"));
    assert!(output.code.contains("consolum:hauri"));
    assert!(
        output.code.contains("consolum:desinit"),
        "expected console readers to probe consolum:desinit before reading:\n{}",
        output.code
    );
    assert!(output.code.contains("solum:lege"));
    assert!(output.code.contains("faber::frame::sermo_open"));
    assert!(!output.code.contains("norma::console::read_line"));
    assert!(!output.code.contains("norma::console::read_bytes"));
    assert!(!output.code.contains("crate::norma::hal::console"));
    assert!(!output.code.contains("builtin-claim"));
    assert!(!output.code.contains("builtin-claimed"));
    assert!(!output.code.contains("println!"));
}

#[test]
#[ignore = "runner serves consolum:hauri through faber-leaf process stdin, so BufferHost.with_stdin_bytes is never delivered; at EOF the empty read faults ERR005 IoFault.InvalidArgument on write"]
fn consolum_eot4_input_interpreted_stdin_roundtrip() {
    let dir = test_temp_dir("consolum-eot4-input-run");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:console" const read_line, read_bytes, write_line, write_bytes
import from "norma:fault" fault

fn echo_line(string msg) → void ⇥ fault.IoFault {
  write_line(msg)
}

fn echo_bytes(list<b8> data) → void ⇥ fault.IoFault {
  write_bytes(data)
}

fn relay() → void ⇥ fault.IoFault {
  echo_line(read_line() coalesce "")
  echo_bytes(read_bytes(4) coalesce |00|)
  echo_line("")
}

main {
  relay()
}
"#,
    )
    .expect("write interpreted consolum input fixture");

    let mut host = BufferHost::default()
        .with_stdin_lines(["salve"])
        .with_stdin_bytes([66, 89, 84, 69]);
    let result = run_mir_for_faber(&Config::default(), &entry, &mut host);
    assert!(
        result.is_ok(),
        "expected consolum input round-trip run success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec!["salve".to_owned(), "BYTE".to_owned()]
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_fs_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-solum-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs" fs as terra

async_main {
  let _ exists ← terra.exists(".")
  let _ parent ← terra.parent("a/b")
  await terra.write_bytes_async("fundet.bin", |41 42 43|)
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:fs package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("solum:exstat"));
    assert!(output.code.contains("solum:parens"));
    assert!(output.code.contains("solum:funde"));
    assert!(output.code.contains("faber::frame::sermo_open"));
    assert!(output.code.contains("fn exists"));
    assert!(output.code.contains("crate::fs::write_bytes_async"));
    assert!(output.code.contains(".await"));
    assert!(!output.code.contains("solum:fundet"));
    assert!(!output.code.contains("norma::fs::exists"));
    assert!(!output.code.contains("norma::fs::parent"));
    assert!(!output.code.contains("crate::norma::hal::fs"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_process_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-processus-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:process" process as proc

main {
  let _ out ← proc.exec("echo hi")
  let _ env ← proc.read_env("PATH")
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:process package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("processus:exsequi"));
    assert!(output.code.contains("processus:lege"));
    assert!(output.code.contains("faber::frame::sermo_open"));
    assert!(output.code.contains("fn exec"));
    // core-norma N4b: `process.spawn` is a Norma body over `processus:genera`
    // (it returns the child pid), so it is emitted like `exec`.
    assert!(output.code.contains("pub(crate) fn spawn"));
    assert!(output.code.contains("processus:genera"));
    assert!(!output.code.contains("norma::process::exec"));
    assert!(!output.code.contains("norma::process::read_env"));
    assert!(!output.code.contains("crate::norma::hal::process"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_time_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-tempus-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:time" time

async_main {
  let _ now ← time.now()
  let _ mono ← time.monotonic()
  let _ active ← time.active()
  let _ ms ← time.MILLISECOND()
  await time.sleep_async(0)
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:time package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("tempus:nunc"));
    assert!(output.code.contains("tempus:monotonicum"));
    assert!(output.code.contains("tempus:activum"));
    assert!(output.code.contains("faber::frame::sermo_open"));
    assert!(output.code.contains("fn now"));
    assert!(output.code.contains("crate::time::sleep_async(0).await"));
    assert!(!output.code.contains("crate::time::sleep_async(0).expect"));
    assert!(!output.code.contains("norma::time::now"));
    assert!(!output.code.contains("norma::time::monotonic"));
    assert!(!output.code.contains("norma::time::millisecond"));
    assert!(!output.code.contains("crate::norma::hal::time"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_builtin_norma_random_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-aleator-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:random" random

main {
  let _ f ← random.random_float()
  let _ n ← random.random_int(1, 6)
  let _ id ← random.uuid()
  let _ bytes ← random.random_bytes(8)
  random.seed(42)
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:random package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("aleator:fractum"));
    assert!(output.code.contains("aleator:sortire"));
    assert!(output.code.contains("aleator:octetos"));
    assert!(output.code.contains("aleator:uuid"));
    assert!(output.code.contains("aleator:semina"));
    assert!(output.code.contains("faber::frame::sermo_open"));
    assert!(output.code.contains("fn random_float"));
    assert!(!output.code.contains("norma::random::random_float"));
    assert!(!output.code.contains("norma::random::random_int"));
    assert!(!output.code.contains("norma::random::uuid"));
    assert!(!output.code.contains("crate::norma::hal::random"));
}

#[test]
fn compile_package_refuses_removed_norma_yaml_module() {
    let dir = test_temp_dir("norma-yaml-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:yaml" yaml

main {}
"#,
    )
    .expect("write entry");

    // Q2: yaml left core. The specifier is an unknown library module.
    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    let unknown_module = result
        .diagnostics
        .iter()
        .find(|diag| diagnostic_has_issue(diag, "unknown_library_module"))
        .expect("unknown built-in library module diagnostic");
    assert!(diagnostic_has_arg(
        unknown_module,
        "specifier",
        "norma:yaml"
    ));
    let known_modules = unknown_module
        .args
        .iter()
        .find(|arg| arg.name == "known_modules")
        .map(|arg| arg.value.as_str())
        .expect("known_modules arg");
    assert!(!known_modules.contains("yaml"));
}

#[test]
fn compile_package_rejects_removed_norma_time_method_calls() {
    let dir = test_temp_dir("norma-tempus-removed-method");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:time" time

main {
  time.siste(1)
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);

    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diag.is_error()
            && diagnostic_has_issue(diag, "namespace_missing_export")
            && diagnostic_has_arg(diag, "member", "siste")
            && diag
                .source_line
                .as_deref()
                .is_some_and(|line| line.contains("siste"))
    }));
}

#[test]
fn compile_package_rejects_removed_norma_vector_placeholder_calls() {
    for (method, args) in [("crea", "[0]"), ("shuffle", "null, [0]")] {
        let dir = test_temp_dir(&format!("norma-vector-removed-{method}"));
        let entry = dir.join("main.fab");
        fs::write(
            &entry,
            format!(
                r#"
import from "norma:vector" vector

main {{
  vector.{method}({args})
}}
"#
            ),
        )
        .expect("write entry");

        let result = compile_with_faber_seams(&Config::default(), &entry);

        assert!(result.output.is_none());
        assert!(
            result.diagnostics.iter().any(|diag| diag.is_error()
                && diagnostic_has_issue(diag, "namespace_missing_export")
                && diagnostic_has_arg(diag, "member", method)
                && diag
                    .source_line
                    .as_deref()
                    .is_some_and(|line| line.contains(method))),
            "expected removed vector.{method} namespace-export diagnostic, got {:?}",
            result.diagnostics
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_fixture_runs_norma_http_runtime_against_local_server() {
    let dir = test_temp_dir("norma-http-fixture");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "norma-http-fixture"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        dir.join("src/main.fab"),
        r#"
import from "norma:http" http

async_main {
    await_const _ responsum ← http.get("http://127.0.0.1:9/test")
    print responsum.status
    let _ caput ← responsum.header("x-faber-test")
    if caput ≡ null { print "header:missing" } else { print "header:present" }
    print responsum.body
}
"#,
    )
    .expect("entry");

    let layout = discover_build_layout(&dir).expect("layout");
    let compile_result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(
        compile_result.success(),
        "expected HTTP package compile success, got {:?}",
        compile_result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = compile_result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("crate::http::get"));
    assert!(output.code.contains("faber::r#gen::block_on"));
    assert!(
        output.code.contains("crate::http::response::Response"),
        "expected leaf Response path crate::http::response::Response, got:\n{}",
        output.code
    );
    assert!(
        output.code.contains(".status") && output.code.contains(".body"),
        "expected field reads of status/body on the Response carrier, got:\n{}",
        output.code
    );
    assert!(
        output.code.contains(".header("),
        "expected header() method read on the Response carrier, got:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("body_json")
            && !output.code.contains(".status()")
            && !output.code.contains(".body()"),
        "H1 Response has fields status/body and header(); no body_json/status()/body():\n{}",
        output.code
    );
    assert!(
        !output.code.contains(&["Faber", "Value"].concat()),
        "generated code must not reference legacy valor type:\n{}",
        output.code
    );

    emit_generated_crate(
        &layout,
        &output.code,
        Some(&read_manifest(&layout.manifest_path).expect("manifest")),
    )
    .expect("emit generated crate");
}

#[test]
fn library_resolver_discovers_builtin_norma_modules_without_allowlist() {
    let resolved = crate::library::default_library_resolver()
        .resolve("norma:fs")
        .expect("resolve should not fail")
        .expect("norma:fs should resolve");

    assert_eq!(resolved.package, "norma");
    assert_eq!(resolved.module_path, vec!["fs"]);
    assert!(resolved.interface_path.ends_with("norma/src/fs.fab"));
}

#[test]
fn library_resolver_rejects_retired_norma_hal_paths() {
    let err = crate::library::default_library_resolver()
        .resolve("norma:hal/solum")
        .expect_err("norma:hal/solum should be rejected");

    assert!(matches!(
        err,
        LibraryResolveError::ModuleUnknown {
            specifier,
            package,
            ..
        } if specifier == "norma:hal/solum" && package == "norma"
    ));
}

#[test]
fn check_package_typechecks_builtin_library_file_imports_against_interfaces() {
    let dir = test_temp_dir("norma-json-solve-interface");
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "norma-json-solve-interface"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    let entry = dir.join("src/main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:json/solve" solve

main {
  solve.nonexistent("{}")
}
"#,
    )
    .expect("write entry");

    let diagnostics = check_with_faber_seams(&Config::default(), &dir.join("faber.toml"));
    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "namespace_missing_export")
            && diagnostic_has_arg(diag, "member", "nonexistent")
    }));
}

#[test]
fn check_package_resolves_wildcard_library_import_bindings() {
    // `public * as <alias>` library imports must resolve in the check path the
    // same way the driver/emit path handles them (PKG001 regression guard).
    let dir = test_temp_dir("wildcard-lib-import-check");
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "wildcard-lib-import-check"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    let entry = dir.join("src/main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs/path" public * as path

main {
    let string name ← path.file_name("/usr/bin/faber")
    print name
}
"#,
    )
    .expect("write entry");

    let diagnostics = check_with_faber_seams(&Config::default(), &dir.join("faber.toml"));
    assert!(
        !diagnostics.iter().any(radix::Diagnostic::is_error),
        "expected wildcard library import to resolve in check, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn compile_package_rejects_faber_kernel_imports() {
    let dir = test_temp_dir("faber-kernel-package");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "faber:json" json
main {}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "kernel_import_script_mode_only"))
    );
}

#[test]
fn compile_package_reports_unknown_library_modules() {
    let dir = test_temp_dir("norma-nope");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:nope" nope
main {}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    let unknown_module = result
        .diagnostics
        .iter()
        .find(|diag| diagnostic_has_issue(diag, "unknown_library_module"))
        .expect("unknown built-in library module diagnostic");
    assert!(diagnostic_has_arg(
        unknown_module,
        "specifier",
        "norma:nope"
    ));
    let known_modules = unknown_module
        .args
        .iter()
        .find(|arg| arg.name == "known_modules")
        .map(|arg| arg.value.as_str())
        .expect("known_modules arg");
    assert!(known_modules.contains("fs"));
    assert!(!known_modules.contains("nope"));
}

#[test]
fn compile_package_rejects_old_norma_slash_library_imports() {
    let dir = test_temp_dir("norma-old-slash");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma/http" http
main {}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "old_builtin_norma_specifier")
            && diagnostic_has_arg(diag, "replacement", "norma:http")
    }));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_keeps_relative_norma_paths_as_local_imports() {
    let dir = test_temp_dir("relative-norma-path");
    fs::create_dir_all(dir.join("norma")).expect("norma dir");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./norma/json" local
main {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("norma/json.fab"),
        r#"
fn salve() → string {
    return "salve"
}
"#,
    )
    .expect("write local module");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected relative local import success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn compile_package_reports_unknown_provider_without_local_fallback() {
    let dir = test_temp_dir("unknown-provider");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "sqlite:client" client
main {}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| diagnostic_has_issue(
        diag,
        "unknown_library_provider"
    ) && diagnostic_has_arg(
        diag, "provider", "sqlite"
    ) && diagnostic_has_arg(
        diag,
        "specifier",
        "sqlite:client"
    )));
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "package_import_unsupported_path"))
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn dual_norma_format_imports_scope_provenance_by_binding() {
    let dir = test_temp_dir("norma-json-csv-provenance");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"import from "norma:csv" csv_mod
import from "norma:json" json_mod

main {
  let list<list<string>> csv_rows ← csv_mod.parse("count,1")
  let _ csv_text ← csv_mod.stringify(csv_rows)
  do {
    let json json_doc ← json_mod.parse("{\"count\": 1}")
    let _ json_text ← json_mod.stringify(json_doc)
  }
  catch err {
    print err
  }
}

"#,
    )
    .expect("write entry");

    let package = analyze_with_faber_seams(&Config::default(), &entry).expect("analyze package");
    let analysis = &package.entry_node().expect("entry unit").analysis;

    let csv_item =
        library_item_by_export(analysis, &["csv"], "stringify", LibraryItemKind::Function);
    let json_item =
        library_item_by_export(analysis, &["json"], "stringify", LibraryItemKind::Function);
    assert_ne!(
        csv_item.def_id, json_item.def_id,
        "overlapping function names must keep distinct DefIds"
    );
    assert_eq!(csv_item.identity.module_path, vec!["csv"]);
    assert_eq!(json_item.identity.module_path, vec!["json"]);
    assert_eq!(csv_item.exported_name, "stringify");
    assert_eq!(json_item.exported_name, "stringify");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected json+csv package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("crate::csv::stringify"));
    assert!(output.code.contains("crate::csv::parse"));
    assert!(output.code.contains("pub(crate) fn stringify"));
    assert!(output.code.contains("pub(crate) fn parse"));
    assert!(!output.code.contains("norma::csv::stringify"));
    assert!(!output.code.contains("norma::csv::parse"));
    assert!(!output.code.contains("norma::json::stringify"));
    assert!(!output.code.contains("norma::json::parse"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_preserves_norma_runtime_types_and_failable_calls() {
    let dir = test_temp_dir("norma-runtime-type-and-failable-calls");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:json" json
import from "norma:value" value

main {
    let json doc ← { "wire_name": "Ada" }
    let string wire ← json.stringify(doc)
    let value tree ← { "wire_name": "Ada" } ↦ value
    let value child ← value.get(tree, "wire_name")
    do {
        let json parsed ← json.parse(wire)
        print parsed, child
    }
    catch err {
        print err
    }
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected typed Norma package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(!output.code.contains("use faber::Valor as valor;"));
    assert!(output.code.contains("let l_tree: faber::Valor"));
    assert!(
        output.code.contains("crate::json::parse") || output.code.contains("json::parse"),
        "expected json::parse in generated code"
    );
    // `value.find` splits its dotted path with the string `split` intrinsic.
    assert!(output.code.contains("path.split("));
}

#[test]
fn aliased_norma_import_preserves_provider_identity_in_analysis() {
    let dir = test_temp_dir("aliased-norma-provider");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:http" http as rete
import from "norma:http/response" response

async_main {
  await_const _ responsum ← rete.get("http://127.0.0.1:9")
}
"#,
    )
    .expect("write entry");

    let package = analyze_with_faber_seams(&Config::default(), &entry).expect("analyze package");
    let analysis = &package.entry_node().expect("entry unit").analysis;

    let binding = library_binding_by_module(analysis, &["http"]);
    assert_eq!(
        binding.identity.provider,
        LibraryProvider::Builtin("norma".to_owned())
    );
    assert_eq!(binding.identity.module_path, vec!["http"]);
    assert!(binding.rust_runtime_module.is_none());

    assert!(
        analysis.hir.items.iter().all(|item| {
            let name = match &item.kind {
                HirItemKind::Interface(interface) => analysis.interner.resolve(interface.name),
                HirItemKind::Struct(strukt) => analysis.interner.resolve(strukt.name),
                HirItemKind::TypeAlias(alias) => analysis.interner.resolve(alias.name),
                _ => return true,
            };
            name != "Response"
        }),
        "library types should not be source-spliced into importer HIR"
    );
    assert!(
        analysis.libraries.items.values().all(|item| {
            !(library_path_eq(&item.identity.module_path, &["http"])
                && item.exported_name == "Response")
        }),
        "Response must not be exported at [http] (no facade interface or type-alias)"
    );
    assert!(
        package
            .entry_node()
            .expect("entry unit")
            .expanded_library_imports
            .iter()
            .any(
                |import| library_path_eq(&import.module.module_path, &["http", "response"])
                    && import.module.package == "norma"
            ),
        "rete import closure must include the Response leaf at [http, response]"
    );
    let item = library_item_by_export(
        analysis,
        &["http", "response"],
        "Response",
        LibraryItemKind::Struct,
    );
    assert_eq!(
        item.identity.provider,
        LibraryProvider::Builtin("norma".to_owned())
    );
    assert_eq!(item.identity.module_path, vec!["http", "response"]);
    assert_eq!(item.exported_name, "Response");
    assert_eq!(item.kind, LibraryItemKind::Struct);
    assert_eq!(item.rust_runtime_type.as_deref(), None);
    assert!(!item.elide_rust_decl);
}

#[cfg(feature = "hir-rust")]
#[test]
fn aliased_norma_http_import_lowers_by_provider_identity() {
    let dir = test_temp_dir("aliased-norma-http-lowering");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:http" http as rete

async_main {
  await_const _ responsum ← rete.get("http://127.0.0.1:9")
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected aliased HTTP package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(
        output
            .code
            .contains("crate::http::get(\"http://127.0.0.1:9\".to_string()).await")
    );
    assert!(
        output
            .code
            .contains("let _: crate::http::response::Response ="),
        "expected the typed leaf struct crate::http::response::Response on the \
         await binding (cf. crate::caelum::terminus::Endpoint); the name itself \
         collapses to `_` for an unused binding per SCR-09P-04, got:\n{}",
        output.code
    );
    assert!(
        output.code.contains("pub struct Response"),
        "Response must lower as a leaf struct, got:\n{}",
        output.code
    );
    assert!(!output.code.contains("rete.get"));
    assert!(
        !output.code.contains("Box<dyn crate::http::Response>")
            && !output.code.contains("pub trait Response"),
        "H1 forbids Box<dyn crate::http::Response> and pub trait Response:\n{}",
        output.code
    );
}

#[test]
fn resolved_library_module_shape_can_describe_future_sqlite_without_rust_metadata() {
    let module = ResolvedLibraryModule::new(
        "sqlite",
        vec!["transactio".to_owned()],
        "/tmp/faber-libs/sqlite/transactio.fab",
    );

    assert_eq!(module.package, "sqlite");
    assert_eq!(module.module_path, vec!["transactio"]);
    assert_eq!(module.module_name(), Some("transactio"));
    assert!(module.interface_path.ends_with("sqlite/transactio.fab"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_resolves_relative_input_from_current_working_directory() {
    let dir = test_temp_dir("relative-input");
    let project_dir = dir.join("project");
    fs::create_dir_all(&project_dir).expect("create project dir");
    fs::write(project_dir.join("main.fab"), "main { print \"salve\" }").expect("write entry");

    let original_cwd = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&dir).expect("set current dir");

    let result = compile_with_faber_seams(&Config::default(), Path::new("./project/main.fab"));

    std::env::set_current_dir(original_cwd).expect("restore current dir");

    assert!(
        result.success(),
        "expected relative package compile success"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_mounts_wildcard_imported_cli_commands() {
    let dir = test_temp_dir("cli-mount");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "config/set"
@ alias "set"
@ operand string name
fn set_config() args args {
  print args.name
}
"#,
    )
    .expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(
        result.success(),
        "expected mounted package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("struct CliArgsJobsConfigSet"));
    // Function signature format may vary with annotation handling changes.
    assert!(output.code.contains("fn set_config"));
    assert!(output.code.contains("jobs::set_config(args);"));
    assert!(output.code.contains("Usage: tool jobs config set"));
    assert!(
        output
            .code
            .contains("command_parts[0] == \"jobs\" && command_parts[1] == \"set\"")
    );
}

#[test]
fn check_package_validates_mounted_cli_commands_without_emitting() {
    let dir = test_temp_dir("check-cli-mount");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
fn run() args args {
  print "running"
}
"#,
    )
    .expect("write jobs");

    let diagnostics = check_with_faber_seams(&Config::default(), &dir);

    assert!(
        !diagnostics.iter().any(Diagnostic::is_error),
        "expected package check success, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_mounted_handlers_can_access_root_globals() {
    let dir = test_temp_dir("cli-mount-root-global");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ option verbose long "verbose" type bool global
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
fn run() args args {
  print args.verbose
}
"#,
    )
    .expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(
        result.success(),
        "expected mounted handler to see root globals, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub verbose: bool"));
    assert!(
        output.code.contains(
            "println!(\"{}\", faber::display::bivalens_with(p_args.verbose, &faber::display::DisplayTokens { true_: \"true\", false_: \"false\", none: \"none\", tuple: \"tuple\" }));"
        ),
        "{}",
        output.code
    );
}

#[test]
fn compile_package_rejects_mounted_local_binding_collision_with_root_global() {
    let dir = test_temp_dir("cli-mount-root-global-collision");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ option verbose long "verbose" type bool global
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "run"
@ option verbose long "local-verbose"
fn run() {}
"#,
    )
    .expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "local_option_global_collision"))
    );
}

#[test]
fn compile_package_rejects_named_import_mount_targets() {
    let dir = test_temp_dir("cli-mount-named");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" set_config as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        "@ command \"run\"\nfn set_config() {}",
    )
    .expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| diagnostic_has_issue(
        diag,
        "mount_requires_wildcard_alias"
    ) && diagnostic_has_arg(diag, "alias", "jobs")));
}

#[test]
fn compile_package_rejects_mounted_global_options() {
    let dir = test_temp_dir("cli-mount-global");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        "@ command \"run\"\n@ option verbose long \"verbose\" global\nfn run() {}",
    )
    .expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "global_option_placement"))
    );
}

#[test]
fn compile_package_rejects_mounted_command_path_collisions() {
    let dir = test_temp_dir("cli-mount-collision");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}

@ command "jobs/run"
fn root_run() {}
"#,
    )
    .expect("write entry");
    fs::write(dir.join("jobs.fab"), "@ command \"run\"\nfn run() {}").expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| diagnostic_has_issue(
        diag,
        "duplicate_command_path"
    ) && diagnostic_has_arg(
        diag, "path", "jobs/run"
    )));
}

#[test]
fn compile_package_rejects_mounted_alias_collisions() {
    let dir = test_temp_dir("cli-mount-alias-collision");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ command "one"
@ alias "same"
fn one() {}

@ command "two"
@ alias "same"
fn two() {}
"#,
    )
    .expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| diagnostic_has_issue(
        diag,
        "duplicate_command_alias"
    ) && diagnostic_has_arg(
        diag,
        "alias",
        "jobs/same"
    )));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_does_not_expose_unmounted_imported_cli_modules() {
    let dir = test_temp_dir("cli-unmounted");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(dir.join("jobs.fab"), "@ command \"run\"\nfn run() {}").expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.success(), "expected package compile success");
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(!output.code.contains("jobs::run"));
    assert!(output.code.contains("Usage: tool"));
    assert!(!output.code.contains("<COMMAND>"));
}

#[test]
fn compile_package_rejects_import_cycles() {
    let dir = test_temp_dir("import-cycle");
    fs::write(
        dir.join("main.fab"),
        "import from \"./jobs\" * as jobs\nmain {}",
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        "import from \"./main\" * as main\nfn run() {}",
    )
    .expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "package_import_cycle"))
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_supports_manifest_example() {
    let dir = test_temp_dir("manifest");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "main {}").expect("write package entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "manifest-example"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir.join("faber.toml"));
    assert!(result.success(), "expected package compile success");
}

#[test]
fn compile_package_rejects_name_only_binary_manifest_without_entry() {
    let dir = test_temp_dir("minimal-valid");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"ok\" }").expect("write entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "minimal"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "missing_binary_entry"))
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_discovers_faber_toml_from_directory() {
    let dir = test_temp_dir("manifest-dir");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "main { print \"ok\" }").expect("write package entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "manifest-dir"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(
        result.success(),
        "expected package directory compile success"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn binding_manifest_verifies_bodyless_declarations_and_shim() {
    let dir = test_temp_dir("binding-manifest-valid");
    fs::create_dir_all(dir.join("src/sqlite")).expect("create source");
    fs::create_dir_all(dir.join("bindings")).expect("create bindings");
    fs::create_dir_all(dir.join("rust")).expect("create rust shim");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "sqlite"
version = "0.1.0"
edition = "2026"

[library]
provider = "sqlite"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[target.rust]
bindings = "bindings/rust.toml"

[target.rust.dependencies]
rusqlite = "0.32"
"#,
    )
    .expect("write manifest");
    fs::write(
        dir.join("src/sqlite.fab"),
        "@ public\nfn exec(string via) → string\n@ public\nfn local() → string { return \"ok\" }\n",
    )
    .expect("write source");
    fs::write(
        dir.join("rust/shim.rs"),
        "pub fn exec(via: String) -> String { via }\n",
    )
    .expect("write shim");
    fs::write(
        dir.join("bindings/rust.toml"),
        r#"[functions."sqlite:sqlite.exec"]
symbol = "crate::shim::exec"

[shim]
path = "rust/shim.rs"
"#,
    )
    .expect("write bindings");

    let report = verify_library_binding_shapes(&dir, "rust").expect("verify bindings");
    assert_eq!(report.declarations, 2);
    assert_eq!(report.bindings, 1);
    assert_eq!(report.shim, Some(dir.join("rust/shim.rs")));
}

#[test]
fn binding_manifest_requires_bodyless_declaration_binding() {
    let dir = test_temp_dir("binding-manifest-missing");
    fs::create_dir_all(dir.join("src")).expect("create source");
    fs::create_dir_all(dir.join("bindings")).expect("create bindings");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "sqlite"

[library]
provider = "sqlite"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[target.rust]
bindings = "bindings/rust.toml"
"#,
    )
    .expect("write manifest");
    fs::write(
        dir.join("src/sqlite.fab"),
        "@ public\nfn exec(string via) → string\n",
    )
    .expect("write source");
    fs::write(dir.join("bindings/rust.toml"), "").expect("write bindings");

    let diagnostics = verify_library_bindings(&dir, "rust").expect_err("missing binding");
    assert!(
        diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "binding_required_missing"))
    );
}

#[test]
fn binding_manifest_rejects_unknown_declaration_rows() {
    let dir = test_temp_dir("binding-manifest-unknown");
    fs::create_dir_all(dir.join("src")).expect("create source");
    fs::create_dir_all(dir.join("bindings")).expect("create bindings");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "sqlite"

[library]
provider = "sqlite"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[target.rust]
bindings = "bindings/rust.toml"
"#,
    )
    .expect("write manifest");
    fs::write(
        dir.join("src/sqlite.fab"),
        "fn local() → string { return \"ok\" }\n",
    )
    .expect("write source");
    fs::write(
        dir.join("bindings/rust.toml"),
        r#"[functions."sqlite:sqlite.missing"]
symbol = "crate::shim::missing"
"#,
    )
    .expect("write bindings");

    let diagnostics = verify_library_bindings(&dir, "rust").expect_err("unknown binding");
    assert!(
        diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "binding_unknown_declaration"))
    );
}

#[test]
fn read_manifest_applies_default_paths_and_build_values() {
    let dir = test_temp_dir("manifest-defaults");
    let manifest = dir.join("faber.toml");
    fs::write(
        &manifest,
        r#"
[package]
name = "defaults"
"#,
    )
    .expect("write manifest");

    let manifest = read_manifest(&manifest).expect("read manifest");
    assert_eq!(manifest.package.name, "defaults");
    assert_eq!(manifest.package.version, "0.1.0");
    assert_eq!(manifest.package.edition, "2026");
    assert_eq!(manifest.paths.source, "src");
    assert_eq!(manifest.paths.entry, None);
    assert!(manifest.library.is_none());
    // Implicit portable default: no explicit [build] target in the manifest.
    assert_eq!(manifest.build.target, None);
    assert_eq!(manifest.build.targets, [] as [String; 0]);
    assert_eq!(manifest.build.kind, "bin");
    assert!(manifest.product.is_none());
    assert!(manifest.locale.locale.is_none());
    assert!(manifest.locale.pack.is_none());
}

#[test]
fn read_manifest_rejects_nonexistent_path() {
    let dir = test_temp_dir("manifest-missing");
    let missing = dir.join("faber.toml");
    let err = read_manifest(&missing).expect_err("non-existent manifest should fail");
    assert_eq!(err.phase, DiagnosticPhase::Io);
    assert!(err.is_error());
}

#[test]
fn g10_web_product_manifest_accepts_browser_app_recipe_without_web_target() {
    let dir = test_temp_dir("g10-web-product-manifest");
    let manifest = dir.join("faber.toml");
    fs::write(
        &manifest,
        r#"
[package]
name = "browser-product"

[paths]
entry = "main.fab"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "pages"
styles = "styles"
public = "public"
controllers_json = "controllers.json"
"#,
    )
    .expect("write manifest");

    let manifest = read_manifest(&manifest).expect("read manifest");
    validate_manifest(&manifest, &dir.join("faber.toml")).expect("product recipe validates");
    assert_eq!(manifest.build.target.as_deref(), Some("ts"));
    let product = manifest.product.expect("product table");
    assert_eq!(product.kind, ManifestProductKind::BrowserApp);
    assert_eq!(product.emit, ManifestProductEmit::TypeScript);
    assert_eq!(product.out, "dist");
}

#[test]
fn g10_web_product_manifest_rejects_unknown_and_unsafe_packaging_fields() {
    let dir = test_temp_dir("g10-web-product-bad-manifest");
    let unknown = dir.join("unknown.toml");
    fs::write(
        &unknown,
        r#"
[package]
name = "bad-product"

[paths]
entry = "main.fab"

[build]
target = "ts"

[product]
kind = "browser-app"
emit = "typescript"
bundler = "vite"
"#,
    )
    .expect("write unknown manifest");
    let err = read_manifest(&unknown).expect_err("unknown product fields fail closed");
    assert!(diagnostic_has_issue(&err, "invalid_package_manifest"));

    let traversal = dir.join("traversal.toml");
    fs::write(
        &traversal,
        r#"
[package]
name = "bad-path"

[paths]
entry = "main.fab"

[build]
target = "ts"

[product]
kind = "browser-app"
emit = "typescript"
out = "../dist"
"#,
    )
    .expect("write traversal manifest");
    let manifest = read_manifest(&traversal).expect("read traversal manifest");
    let err = validate_manifest(&manifest, &traversal).expect_err("unsafe product path rejects");
    assert!(diagnostic_has_issue(&err, "invalid_product_path"));
}

#[test]
fn read_manifest_accepts_source_library_without_entry() {
    let dir = test_temp_dir("manifest-library");
    let manifest = dir.join("faber.toml");
    fs::write(
        &manifest,
        r#"
[package]
name = "norma"
version = "0.1.0"

[library]
provider = "norma"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("write manifest");

    let manifest = read_manifest(&manifest).expect("read manifest");
    assert_eq!(manifest.package.name, "norma");
    assert_eq!(
        manifest
            .library
            .as_ref()
            .map(|library| library.provider.as_str()),
        Some("norma")
    );
    assert_eq!(manifest.paths.source, "src");
    assert_eq!(manifest.paths.entry, None);
    assert_eq!(manifest.build.kind, "lib");
    assert_eq!(manifest.build.targets, vec!["rust".to_owned()]);
}

#[test]
fn discover_package_accepts_library_manifest_without_entry() {
    let dir = test_temp_dir("manifest-library-discover");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "libpkg"

[library]
provider = "libpkg"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("write manifest");

    let spec = discover_package(&dir).expect("discover library manifest");
    assert_eq!(spec.source_root, src);
    assert_eq!(spec.entry, spec.source_root);
}

#[test]
fn validate_manifest_rejects_nested_source_root() {
    let dir = test_temp_dir("manifest-nested-source-root");
    let manifest_path = dir.join("faber.toml");
    fs::write(
        &manifest_path,
        r#"
[package]
name = "nested-root"

[paths]
source = "components/faber"
"#,
    )
    .expect("write manifest");

    let manifest = read_manifest(&manifest_path).expect("read manifest");
    let err = validate_manifest(&manifest, &manifest_path)
        .expect_err("nested source root must be rejected");
    assert!(diagnostic_has_issue(
        &err,
        "package_member_unsupported_source_root"
    ));
    assert!(
        diagnostic_has_arg(&err, "source", "components/faber"),
        "diagnostic must name the rejected source root, got: {err:?}"
    );
    assert!(
        diagnostic_has_arg(&err, "supported_source_roots", "src,."),
        "diagnostic must name the supported default and package-root values, got: {err:?}"
    );
    assert!(
        diagnostic_has_arg(&err, "reason", "custom_source_root_usage_contract_pending"),
        "diagnostic must name the pending usage contract, got: {err:?}"
    );
}

#[test]
fn validate_manifest_rejects_custom_depth1_source_root() {
    let dir = test_temp_dir("manifest-custom-depth1-source");
    let manifest_path = dir.join("faber.toml");
    fs::write(
        &manifest_path,
        r#"
[package]
name = "custom-root"

[paths]
source = "lib"
"#,
    )
    .expect("write manifest");

    let manifest = read_manifest(&manifest_path).expect("read manifest");
    let err =
        validate_manifest(&manifest, &manifest_path).expect_err("depth-1 custom root rejected");
    assert!(diagnostic_has_issue(
        &err,
        "package_member_unsupported_source_root"
    ));
}

#[test]
fn compile_package_rejects_unsupported_source_root_no_compile_proceeds() {
    let dir = test_temp_dir("manifest-unsupported-source");
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "bad-source"

[paths]
source = "components/faber"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(dir.join("src/main.fab"), "main { }\n").expect("write entry");

    let spec = discover_package(&dir)
        .expect_err("discovery must reject an unsupported source root before resolution");
    assert!(diagnostic_has_issue(
        &spec,
        "package_member_unsupported_source_root"
    ));

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "package_member_unsupported_source_root"))
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn discover_package_accepts_package_root_source_dot() {
    let dir = test_temp_dir("manifest-dot-source");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "dot-pkg"

[paths]
source = "."
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(dir.join("main.fab"), "main { }\n").expect("write entry");

    let manifest = read_manifest(&dir.join("faber.toml")).expect("read manifest");
    validate_manifest(&manifest, &dir.join("faber.toml"))
        .expect("package-root source is in the supported set");

    let spec = discover_package(&dir).expect("discover dot-source package");
    assert!(spec.manifest_backed);
    assert_eq!(spec.source_root, dir.path().to_path_buf());
    assert_eq!(spec.entry, dir.join("main.fab"));

    // Full compile proceeds with `source = "."` (examples/automation canary shape).
    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(
        result.output.is_some(),
        "dot-source package must compile, got {:?}",
        result.diagnostics
    );
}

#[test]
fn load_package_with_config_surfaces_missing_manifest_after_validation() {
    let dir = test_temp_dir("manifest-missing-after-validation");
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "vanishing"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(dir.join("src/main.fab"), "main { }\n").expect("write entry");

    let spec = discover_package(&dir).expect("discover validated package");
    assert!(spec.manifest_backed);
    fs::remove_file(dir.join("faber.toml")).expect("delete manifest after discovery");

    let resolver = library_resolver_from_config(&Config::default());
    let Err(err) = load_with_faber_seams(&spec, &resolver, None, false, None) else {
        panic!("missing manifest after validation must be a diagnostic, not a silent default");
    };
    assert!(
        err.iter()
            .any(|d| diagnostic_has_issue(d, "package_manifest_missing_after_validation"))
    );
}

#[test]
fn load_package_with_config_rejects_malformed_manifest() {
    let dir = test_temp_dir("manifest-malformed-load");
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(dir.join("faber.toml"), "this is not toml [").expect("write malformed manifest");
    fs::write(dir.join("src/main.fab"), "main { }\n").expect("write entry");

    // Discovery itself fails loudly on a malformed manifest.
    let err = discover_package(&dir).expect_err("malformed manifest must fail at discovery");
    assert!(diagnostic_has_issue(&err, "invalid_package_manifest"));

    // A manifest-backed spec must not silently swallow a malformed manifest either.
    let spec = radix::program::discovery::PackageSpec {
        package_root: dir.path().to_path_buf(),
        source_root: dir.join("src"),
        entry: dir.join("src/main.fab"),
        templates: BTreeMap::default(),
        manifest_backed: true,
    };
    let resolver = library_resolver_from_config(&Config::default());
    let Err(err) = load_with_faber_seams(&spec, &resolver, None, false, None) else {
        panic!("malformed manifest must fail loudly on load, not be swallowed");
    };
    assert!(
        err.iter()
            .any(|d| diagnostic_has_issue(d, "invalid_package_manifest"))
    );
}

#[test]
fn manifestless_legacy_spec_loads_without_manifest() {
    let dir = test_temp_dir("legacy-manifestless-load");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { }\n").expect("write entry");

    let spec = discover_package(&entry).expect("discover legacy file input");
    assert!(!spec.manifest_backed);

    let resolver = library_resolver_from_config(&Config::default());
    let files = load_with_faber_seams(&spec, &resolver, None, false, None)
        .expect("legacy manifestless load keeps working");
    assert_eq!(files.len(), 1);
}

#[test]
fn compile_package_rejects_binary_manifest_without_entry() {
    let dir = test_temp_dir("manifest-bin-missing-entry");
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "missing-entry"

[paths]
source = "src"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "missing_binary_entry"))
    );
}

#[test]
fn compile_package_rejects_invalid_manifest_provider_names() {
    let dir = test_temp_dir("manifest-invalid-provider");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "bad provider"

[library]
provider = "bad/provider"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "invalid_package_name"))
    );
}

#[test]
fn compile_package_rejects_invalid_manifest_library_provider() {
    let dir = test_temp_dir("manifest-invalid-library-provider");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "libpkg"

[library]
provider = "bad/provider"

[build]
kind = "lib"
targets = ["rust"]
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "invalid_library_provider"))
    );
}

#[test]
fn compile_package_rejects_library_manifest_without_targets() {
    let dir = test_temp_dir("manifest-lib-missing-targets");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "libpkg"

[library]
provider = "libpkg"

[build]
kind = "lib"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "missing_library_targets"))
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_uses_manifest_reader_locale_default_pack() {
    let dir = test_temp_dir("manifest-reader-locale");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "入口 { 输出 \"ok\" }").expect("write package entry");
    write_zh_locale_pack(&dir, "zh-Hans.toml");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "reader-locale"

[paths]
source = "src"
entry = "main.fab"

[reader]
locale = "zh-Hans"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);

    assert!(
        result.success(),
        "expected manifest reader locale package compile success, got {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_uses_installed_reader_locale_pack_without_package_pack() {
    let dir = test_temp_dir("manifest-installed-reader-locale");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "入口 { 显示 \"ok\" }").expect("write package entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "installed-reader-locale"

[paths]
source = "src"
entry = "main.fab"

[reader]
locale = "zh-Hans"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);

    assert!(
        result.success(),
        "expected installed reader locale package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert!(
        result.diagnostics.is_empty(),
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn installed_reader_locale_reference_examples_compile_from_installed_packs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus-product/reader-locale");

    // zh-Hant, ar, hi, vi removed: reader locale changes now
    // produce LOCALE002 lexer warnings before semantic analysis.
    // Only zh-Hans still compiles cleanly.
    let (locale, path, function, binding, greeting) = (
        "zh-Hans",
        "zh-Hans",
        "fn 迎接(p_访客: String) -> String",
        "let l_欢迎语: String",
        "欢迎光临",
    );
    let example = root.join(path);
    let result = compile_with_faber_seams(&Config::default(), &example);

    assert!(
        result.success(),
        "expected installed {locale} reader locale example compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert!(
        !result.diagnostics.iter().any(Diagnostic::is_error),
        "unexpected {locale} error diagnostics: {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected {locale} generated Rust output");
    };
    let rust = output.code;

    assert!(rust.contains(function), "{locale} Rust output:\n{rust}");
    assert!(rust.contains(binding), "{locale} Rust output:\n{rust}");
    assert!(rust.contains(greeting), "{locale} Rust output:\n{rust}");
    assert!(rust.contains("println!"), "{locale} Rust output:\n{rust}");
}

#[test]
fn package_check_reads_la_library_under_its_own_locale() {
    // SEM004 library-locale propagation (web-mig U4/U5 unblocker): a library
    // module resolved through a consumer's lock must be checked under the
    // LIBRARY's own locale — canonical spellings accepted — never the
    // consumer's `[reader] locale` and never the faber product default
    // (`DEFAULT_CODE_LOCALE = "en"`). A Latin-surface library with no declared
    // locale is read under the Latin identity surface, so its canonical-Latin
    // intrinsic call (`longitudo`) typechecks even when an en consumer
    // imports it.
    let dir = test_temp_dir("la-library-en-consumer");
    let lib_root = dir.join("lib");
    fs::create_dir_all(lib_root.join("lalib/src")).expect("create library src");
    fs::write(
        lib_root.join("lalib/faber.toml"),
        r#"[package]
name = "lalib"
version = "0.1.0"
edition = "2026"

[library]
provider = "lalib"

[paths]
source = "src"

[build]
kind = "lib"
target = "ts"
targets = ["ts"]
[reader]
locale = "la"
"#,
    )
    .expect("write library manifest");
    fs::write(
        lib_root.join("lalib/src/math.fab"),
        r#"+++
locale = "la"
+++
# Public import: importa ex "lalib:math" math

@ publica
genus Vector3 {
    varia lista<numerus> elementa

    functio validum() → bivalens {
        redde ego.elementa.longitudo() ≡ 3
    }
}

@ publica
functio make_vector() → Vector3 {
    redde Vector3 { elementa = [1, 2, 3] }
}
"#,
    )
    .expect("write library module");

    let consumer = dir.join("consumer");
    fs::create_dir_all(consumer.join("src")).expect("create consumer src");
    fs::write(
        consumer.join("faber.toml"),
        r#"[package]
name = "la-library-en-consumer"
version = "0.1.0"
edition = "2026"

[paths]
entry = "src/main.fab"

[build]
target = "ts"
kind = "bin"

[dependencies]
lalib = "0.1.0"

[reader]
locale = "en"
"#,
    )
    .expect("write consumer manifest");
    fs::write(
        consumer.join("faber.lock"),
        format!(
            r#"
[[package]]
name = "lalib"
version = "0.1.0"
source = "path"
package_root = "{}"
kind = "lib"
target_language = "ts"
target_triple = "browser"
target_manifest = ""
interface_root = "{}"
artifact = ""
crate = "lalib"
rustc = ""
"#,
            lib_root.join("lalib").display(),
            lib_root.join("lalib/src").display(),
        ),
    )
    .expect("write lock");
    fs::write(
        consumer.join("src/main.fab"),
        r#"import from "lalib:math" math

fn probe() → bool {
    return math.make_vector().validum()
}
"#,
    )
    .expect("write consumer entry");

    let config = Config::default().with_library_home(lib_root);
    let diagnostics = check_with_faber_seams(&config, &consumer.join("src/main.fab"));

    assert!(
        !diagnostics.iter().any(Diagnostic::is_error),
        "expected the la library to typecheck under its own locale, got {:?}",
        diagnostics
            .iter()
            .filter(|diag| diag.is_error())
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_uses_manifest_locale_pack_path() {
    let dir = test_temp_dir("manifest-reader-pack");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "入口 { 输出 \"ok\" }").expect("write package entry");
    write_zh_locale_pack(&dir, "custom.toml");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "reader-pack"

[paths]
source = "src"
entry = "main.fab"

[reader]
locale = "zh-Hans"
pack = "./locale/custom.toml"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn package_reader_locale_cli_selection_overrides_manifest_locale() {
    let dir = test_temp_dir("reader-cli-override");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "入口 { 输出 \"ok\" }").expect("write package entry");
    write_zh_locale_pack(&dir, "zh-Hans.toml");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "reader-cli-override"

[paths]
source = "src"
entry = "main.fab"

[reader]
locale = "th-TH"
"#,
    )
    .expect("write manifest");

    let (config, pack) =
        config_with_locale(Target::HirRust, &dir, Some("zh-Hans"), None).expect("reader config");
    let pack = pack.expect("reader pack");

    assert_eq!(pack.metadata.id, "zh-Hans");
    assert!(config.locale_pack.is_some());
}

#[test]
fn compile_package_rejects_undeclared_latin_keyword_in_grammar_position() {
    let dir = test_temp_dir("manifest-reader-fallback");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "incipit { nota \"ok\" }").expect("write package entry");
    write_zh_locale_pack(&dir, "zh-Hans.toml");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "reader-fallback"

[paths]
source = "src"
entry = "main.fab"

[reader]
locale = "zh-Hans"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);

    // S2-U2 pack-authoritative gate: the zh-Hans pack does not declare the
    // Latin spellings `incipit`/`nota`, so they must NOT claim their grammar
    // slots. The undeclared spellings stay ordinary identifiers and the
    // grammar-position use fails normally — an ordinary undefined-identifier
    // failure (SEM001), with no fallback event and no LOCALE diagnostic.
    assert!(
        !result.success(),
        "undeclared Latin keyword must not claim the grammar slot: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM001")),
        "undeclared spelling must fail as an ordinary undefined identifier: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().all(|diagnostic| !diagnostic
            .code
            .is_some_and(|code| code.starts_with("LOCALE"))),
        "no LOCALE diagnostic may be emitted on the flipped path: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn thai_reader_locale_example_compiles_from_manifest() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus-product/reader-locale/th-TH");

    let result = compile_with_faber_seams(&Config::default(), &example);

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected generated Rust output");
    };
    let rust = output.code;
    assert!(rust.contains("fn ต้อนรับ(p_who: String) -> String"));
    let line_binding = rust
        .lines()
        .find(|l| l.contains("let l_line"))
        .unwrap_or_default()
        .to_owned();
    assert!(line_binding.contains("format!(\"ขอต้อนรับ, {p_who}!\")"));
    assert!(rust.contains("fn สอบผ่าน(p_mark: i64) -> bool"));
    assert!(rust.contains("if p_mark >= 90"));
    assert!(rust.contains("return true;"));
    assert!(rust.contains("return false;"));
    assert!(rust.contains("fn นับคะแนน(p_marks: &Vec<i64>) -> i64"));
    assert!(rust.contains("for l_mark in 0..(((*p_marks)).len() as i64)"));
    assert!(rust.contains("continue;"));
    assert!(rust.contains("break;"));
    assert!(rust.contains("fn ลดลง(p_start: i64) -> i64"));
    assert!(rust.contains("while l_current > 0"));
    assert!(rust.contains("let l_mark: i64 = 77;"));
    assert!(rust.contains("let l_marks: Vec<i64> = vec!["));
    // Display tokens are the Thai pack's own words (`verum`/`falsum`/`iuncta`
    // keywords and the `nihil` type row), emitted as Rust string literals.
    assert!(
        rust.contains(
            r#"faber::display::bivalens_with(สอบผ่าน(l_mark), &faber::display::DisplayTokens { true_: "จร\u{e34}ง", false_: "เท\u{e47}จ", none: "ไม\u{e48}ม\u{e35}", tuple: "ท\u{e39}เพ\u{e34}ล" })"#
        ),
        "{rust}"
    );
    assert!(rust.contains("นับคะแนน(&l_marks)"));
    assert!(rust.contains("ลดลง(4)"));
}

// S09-7: `go` is a supported manifest target; the fail-closed case uses a
// spelling nothing supports.
#[test]
fn compile_package_rejects_unsupported_manifest_target() {
    let dir = test_temp_dir("manifest-target");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "main {}").expect("write package entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "bad-target"

[paths]
entry = "main.fab"

[build]
target = "ada"
"#,
    )
    .expect("write manifest");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| diagnostic_has_issue(
        diag,
        "package_build_target_unsupported"
    ) && diagnostic_has_arg(diag, "target", "ada")));
}

#[test]
fn every_target_resolver_fails_closed_on_unsupported_library_target() {
    // S09-1: `build.kind = "lib"` validators skip the singular `build.target`,
    // so check, build and the executed lane each resolved it separately with
    // different fallbacks. All of them now share one resolver and report the
    // unsupported target.
    let dir = test_temp_dir("lib-target-resolvers");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("lib.fab"), "fn one() -> numerus { redde 1 }\n").expect("write source");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "lib-target"

[build]
kind = "lib"
target = "ada"
targets = ["rust"]

[library]
provider = "lib-target"
"#,
    )
    .expect("write manifest");

    let package_err = super::manifest_build_target_for_input(dir.path())
        .expect_err("package target resolver must fail closed");
    assert!(diagnostic_has_issue(
        &package_err,
        "package_build_target_unsupported"
    ));
    let lane_err = super::executed_lane_analysis_target(&Config::default(), dir.path())
        .expect_err("executed lane resolver must fail closed");
    assert!(diagnostic_has_issue(
        &lane_err,
        "package_build_target_unsupported"
    ));

    let explicit_lane_err = super::executed_lane_analysis_target(
        &Config::default().with_target(Target::MirFmir),
        dir.path(),
    )
    .expect_err("an explicit executed target must not hide a manifest error");
    assert!(diagnostic_has_issue(
        &explicit_lane_err,
        "package_build_target_unsupported"
    ));

    // No manifest: every resolver reports "no manifest" and the caller picks
    // its own named default.
    let bare = test_temp_dir("lib-target-resolvers-bare");
    assert_eq!(
        super::manifest_build_target_for_input(bare.path()).expect("no manifest"),
        None
    );
    assert_eq!(
        super::executed_lane_analysis_target(&Config::default(), bare.path()).expect("no manifest"),
        Target::MirFmirBinary
    );
}

#[test]
fn discover_package_accepts_manifest_package_mir_targets() {
    for target in ["rust", "scena", "fmir-text", "fmir"] {
        let dir = test_temp_dir(&format!("manifest-target-{}", target.replace('-', "_")));
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src");
        fs::write(src.join("main.fab"), "main {}").expect("write package entry");
        fs::write(
            dir.join("faber.toml"),
            format!(
                r#"
[package]
name = "manifest-target"

[paths]
entry = "main.fab"

[build]
target = "{target}"
"#
            ),
        )
        .expect("write manifest");

        discover_package(&dir)
            .unwrap_or_else(|diag| panic!("manifest target {target} should be accepted: {diag:?}"));
    }
}

#[test]
fn compile_package_rejects_nested_module_mounts() {
    let dir = test_temp_dir("mount-cycle");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./jobs" * as jobs

@ cli "tool"
@ commands "jobs" ex jobs
main args args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ commands "again" ex jobs
@ command "run"
fn run() {}
"#,
    )
    .expect("write jobs");

    let result = compile_with_faber_seams(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diag.args
            .contains(&DiagnosticArg::new("issue", "nested_module_mount"))
    }));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_exports_imported_module_functions_to_entry() {
    let dir = test_temp_dir("package-import-function");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./auxilium" aux

main {
    print aux.saluta("Marcus")
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
@ public
fn saluta(string nomen) → string {
    return "Salve, §!"(nomen)
}
"#,
    )
    .expect("write module");

    let result = compile_with_faber_seams(&Config::default(), &dir.join("main.fab"));
    assert!(
        result.success(),
        "expected package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(
        output
            .code
            .contains("crate::auxilium::saluta(\"Marcus\".to_string())")
    );
    assert!(!output.code.contains("use crate::auxilium::aux"));
    assert!(output.code.contains("pub(crate) fn saluta"));
    assert!(!output.code.contains("#![allow(non_snake_case)]"));
    let main_pos = output.code.find("fn main()").expect("entry main");
    let module_pos = output.code.find("pub mod auxilium").expect("module");
    // SCR-09 recorded-suppressions prelude: the emitted carrier always opens
    // with a crate-level `#![allow(...)]` inner attribute, so module blocks
    // precede the entry body (inner attributes must head the file).
    assert!(
        module_pos < main_pos,
        "package modules follow the inner-attribute prelude and precede the entry body. code:\n{}",
        output.code
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_calls_top_level_function_through_file_namespace_alias() {
    let dir = test_temp_dir("package-file-namespace-function");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./auxilium" Aux

main {
    print Aux.adde(2, 3)
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r"
@ public
fn adde(int left, int right) → int {
    return left + right
}
",
    )
    .expect("write module");

    let result = compile_with_faber_seams(&Config::default(), &dir.join("main.fab"));
    assert!(
        result.success(),
        "expected file namespace function package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("crate::auxilium::adde(2, 3)"));
    assert!(!output.code.contains("use crate::auxilium::Aux"));
    assert!(!output.code.contains("Aux.adde"));
}

#[test]
fn check_package_types_top_level_function_through_file_namespace_alias() {
    let dir = test_temp_dir("package-file-namespace-function-type");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./auxilium" Aux

main {
    let string label ← Aux.label("Marcus")
    print label
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
@ public
fn label(string nomen) → string {
    return "Salve, §!"(nomen)
}
"#,
    )
    .expect("write module");

    let diagnostics = check_with_faber_seams(&Config::default(), &entry);
    assert!(
        !diagnostics.iter().any(radix::Diagnostic::is_error),
        "expected typed file namespace function call, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_package_selective_type_import_binds_the_type() {
    // D22.5: a selective import binds a type export; check must close the
    // emit def table over the imported nominal (no DEFID001).
    let dir = test_temp_dir("package-selective-type-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./auxilium" const Canis, Felis as Cattus

fn nomen(Canis c) → string {
    return c.nomen
}

fn felis(Cattus c) → string {
    return c.nomen
}

main {
    let Canis c ← Canis { nomen = "rex" }
    print nomen(c)
    print felis(Cattus { nomen = "tom" })
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r"
@ public
class Canis {
    mut string nomen
}

@ public
class Felis {
    mut string nomen
}
",
    )
    .expect("write module");

    let diagnostics = check_with_faber_seams(&Config::default(), &entry);
    assert!(
        !diagnostics.iter().any(radix::Diagnostic::is_error),
        "expected selective type import to check, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_package_admits_est_against_imported_types() {
    // D23.1/D23.5 (supersedes D18.1): `x is aux.Felis` / `x is Canis` on
    // imported types is a legal type test.
    let dir = test_temp_dir("package-est-imported-type");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./auxilium" aux
import from "./auxilium" const Canis

fn qualified(aux.Felis ∪ aux.Canis x) → bool {
    return x is aux.Felis
}

fn bare(aux.Felis ∪ Canis x) → bool {
    return x is Canis
}

main {
    print qualified(aux.Felis { nomen = "tom" })
    print bare(Canis { nomen = "rex" })
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r"
@ public
class Canis {
    mut string nomen
}

@ public
class Felis {
    mut string nomen
}
",
    )
    .expect("write module");

    let diagnostics = check_with_faber_seams(&Config::default(), &entry);
    assert!(
        !diagnostics.iter().any(radix::Diagnostic::is_error),
        "expected imported-type est targets to check, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_package_rejects_wrong_argument_type_for_file_namespace_alias() {
    let dir = test_temp_dir("package-file-namespace-function-arg-type");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./auxilium" Aux

main {
    print Aux.label(42)
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r"
@ public
fn label(string nomen) → string {
    return nomen
}
",
    )
    .expect("write module");

    let diagnostics = check_with_faber_seams(&Config::default(), &entry);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "argument_type_mismatch")),
        "expected argument mismatch, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_package_types_norma_fs_path_namespace_call() {
    let dir = test_temp_dir("package-file-namespace-solum-path-type");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:fs/path" path

main {
    let string name ← path.file_name("/usr/bin/faber")
    print name
}
"#,
    )
    .expect("write entry");

    let diagnostics = check_with_faber_seams(&Config::default(), &entry);
    assert!(
        !diagnostics.iter().any(radix::Diagnostic::is_error),
        "expected typed fs/path namespace call, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_package_types_top_level_json_solve_namespace_call() {
    let library_home = test_temp_dir("top-level-json-solve-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "json/solve",
        r#"+++
locale = "la"
+++
@ publica
functio solve(textus json) → valor {
    redde json ↦ valor
}
"#,
    );

    let dir = test_temp_dir("top-level-json-solve-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:json/solve" solve

main {
    let value doc ← solve.solve("{}")
    print doc
}
"#,
    )
    .expect("write entry");

    let diagnostics =
        check_with_faber_seams(&config_with_library_home(library_home.path()), &entry);
    assert!(
        !diagnostics.iter().any(radix::Diagnostic::is_error),
        "expected typed top-level json/solve namespace call, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn build_package_routes_duplicate_stem_file_namespace_imports_by_path() {
    let dir = test_temp_dir("package-file-namespace-duplicate-stems");
    fs::create_dir_all(dir.join("a")).expect("create a dir");
    fs::create_dir_all(dir.join("b")).expect("create b dir");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./a/util" A
import from "./b/util" B

main {
    print A.label()
    print B.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("a").join("util.fab"),
        r#"
@ public
fn label() → string {
    return "A"
}
"#,
    )
    .expect("write a util");
    fs::write(
        dir.join("b").join("util.fab"),
        r#"
@ public
fn label() → string {
    return "B"
}
"#,
    )
    .expect("write b util");

    let code = compile_package_rust_code(&entry);

    assert!(
        code.contains("crate::a::util::label()"),
        "expected A import routed through a/util namespace:\n{code}"
    );
    assert!(
        code.contains("crate::b::util::label()"),
        "expected B import routed through b/util namespace:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn build_package_supplies_namespace_metadata_to_non_entry_modules() {
    let dir = test_temp_dir("package-file-namespace-non-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./service" Service

main {
    Service.run()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("service.fab"),
        r#"
import from "./util" U

@ public
fn run() → void {
    print U.adde(2, 3)
}
"#,
    )
    .expect("write service");
    fs::write(
        dir.join("util.fab"),
        r"
@ public
fn adde(int left, int right) → int {
    return left + right
}
",
    )
    .expect("write util");

    let code = compile_package_rust_code(&entry);

    assert!(
        code.contains("pub mod service"),
        "expected generated service module:\n{code}"
    );
    assert!(
        code.contains("crate::util::adde(2, 3)"),
        "expected non-entry module import to route through crate util namespace:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn build_package_keeps_duplicate_qualified_type_names_namespace_scoped() {
    let dir = test_temp_dir("package-file-namespace-duplicate-types");
    fs::create_dir_all(dir.join("a")).expect("create a dir");
    fs::create_dir_all(dir.join("b")).expect("create b dir");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "./a/types" A
import from "./b/types" B

main {
    let A.Thing a ← A.Thing { label = "A" }
    let B.Thing b ← B.Thing { label = "B" }
    print a.label
    print b.label
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("a").join("types.fab"),
        r"
@ public
class Thing {
    mut string label
}
",
    )
    .expect("write a types");
    fs::write(
        dir.join("b").join("types.fab"),
        r"
@ public
class Thing {
    mut string label
}
",
    )
    .expect("write b types");

    let code = compile_package_rust_code(&entry);

    assert!(
        code.contains("crate::a::types::Thing"),
        "expected A.Thing to stay in a/types namespace:\n{code}"
    );
    assert!(
        code.contains("crate::b::types::Thing"),
        "expected B.Thing to stay in b/types namespace:\n{code}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_calls_file_function_through_file_namespace_alias() {
    let dir = test_temp_dir("package-file-namespace-function");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./auxilium" Aux

main {
    print Aux.dupla(4)
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r"
@ public
fn dupla(int n) → int {
    return n * 2
}
",
    )
    .expect("write module");

    let result = compile_with_faber_seams(&Config::default(), &dir.join("main.fab"));
    assert!(
        result.success(),
        "expected file namespace function package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("crate::auxilium::dupla(4)"));
    assert!(!output.code.contains("use crate::auxilium::Aux"));
    assert!(!output.code.contains("Aux.dupla"));
}

#[test]
fn compile_package_rejects_private_function_through_file_namespace_alias() {
    let dir = test_temp_dir("package-private-file-namespace-function");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./auxilium" Aux

main {
    print Aux.secretum()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
@ private
fn secretum() → string {
    return "hidden"
}

fn publicum() → string {
    return secretum()
}
"#,
    )
    .expect("write module");

    let result = compile_with_faber_seams(&Config::default(), &dir.join("main.fab"));
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "import_module_private")
            && diagnostic_has_arg(diag, "name", "secretum")
    }));
}

#[cfg(feature = "hir-rust")]
#[test]
fn compile_package_exports_publica_function_through_file_namespace_alias() {
    let dir = test_temp_dir("package-publica-file-namespace-function");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./auxilium" Aux

main {
    print Aux.publicum()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
@ public
fn publicum() → string {
    return "visible"
}
"#,
    )
    .expect("write module");

    let result = compile_with_faber_seams(&Config::default(), &dir.join("main.fab"));
    assert!(
        result.success(),
        "expected explicit publica function export success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );
}

#[test]
fn compile_package_rejects_private_function_from_parser_file_namespace_alias() {
    let dir = test_temp_dir("package-private-parser-file-namespace");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./parser" P

main {
    print P.scan("{}")
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("parser.fab"),
        r"
@ private
fn scan(string input) → string {
    return input
}

fn solve(string input) → string {
    return input
}
",
    )
    .expect("write module");

    let result = compile_with_faber_seams(&Config::default(), &dir.join("main.fab"));
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "import_module_private")
            && diagnostic_has_arg(diag, "name", "scan")
    }));
}

#[test]
fn compile_package_rejects_private_library_type_through_file_namespace_alias() {
    let library_home = test_temp_dir("private-library-type-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "types",
        r#"+++
locale = "la"
+++
@ privata
genus Secretum {
}

genus Publicum {
}
"#,
    );

    let dir = test_temp_dir("private-library-type-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:types" types

main {
    let types.Secretum secretum ← types.Secretum {}
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&config_with_library_home(library_home.path()), &entry);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| { diagnostic_has_issue(diag, "qualified_type_not_exported") })
    );
}

#[test]
fn compile_package_rejects_private_library_function_through_file_namespace_alias() {
    let library_home = test_temp_dir("private-library-function-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "helpers",
        r#"+++
locale = "la"
+++
@ privata
functio secretum() → textus {
    redde "hidden"
}

functio publicum() → textus {
    redde secretum()
}
"#,
    );

    let dir = test_temp_dir("private-library-function-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:helpers" helpers

main {
    print helpers.secretum()
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&config_with_library_home(library_home.path()), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "namespace_missing_export")
            && diagnostic_has_arg(diag, "member", "secretum")
    }));
}

// ---------------------------------------------------------------------------
// Phase 1: BuildLayout path model tests (pure, no Cargo, sibling contract)
// ---------------------------------------------------------------------------

#[test]
fn build_layout_from_root_produces_sibling_debug_release_and_faber_dirs() {
    let layout = BuildLayout::from_package_root("/tmp/hello-world", "hello-world");

    assert_eq!(
        layout.package_root,
        Path::new("/tmp/hello-world").to_path_buf()
    );
    assert_eq!(
        layout.generated_crate_root,
        Path::new("/tmp/hello-world/target/faber").to_path_buf()
    );
    assert_eq!(
        layout.cargo_target_dir,
        Path::new("/tmp/hello-world/target").to_path_buf()
    );
    assert_eq!(
        layout.debug_binary,
        Path::new("/tmp/hello-world/target/debug/hello-world").to_path_buf()
    );
    assert_eq!(
        layout.release_binary,
        Path::new("/tmp/hello-world/target/release/hello-world").to_path_buf()
    );

    // Critical sibling contract: debug/release are peers of faber/, never under it
    let faber_target = layout.generated_crate_root.join("target");
    assert!(
        !layout.debug_binary.starts_with(&faber_target),
        "debug binary must not live under target/faber/target (would create nested target)"
    );
    assert!(
        !layout.release_binary.starts_with(&faber_target),
        "release binary must not live under target/faber/target"
    );
    assert_eq!(layout.binary_name(), "hello-world");
}

#[test]
fn sanitize_crate_name_handles_mixed_case_punctuation_and_digits() {
    assert_eq!(sanitize_crate_name("My Cool App!"), "my-cool-app");
    assert_eq!(sanitize_crate_name("Faber_Tool-2026"), "faber_tool-2026");
    assert_eq!(sanitize_crate_name("123pkg"), "p-123pkg");
    assert_eq!(sanitize_crate_name(""), "package");
    assert_eq!(sanitize_crate_name("___"), "package");
    assert_eq!(sanitize_crate_name("a/b\\c"), "a-b-c");
}

#[cfg(feature = "hir-rust")]
#[test]
fn discover_build_layout_supports_manifest_file_input() {
    let dir = test_temp_dir("layout-manifest-file");
    let manifest = dir.join("faber.toml");
    fs::write(
        &manifest,
        r#"
[package]
name = "Manifest-Pkg"
version = "0.2.0"
"#,
    )
    .expect("write manifest");

    let layout = discover_build_layout(&manifest).expect("discover from manifest file");
    assert_eq!(layout.binary_name(), "manifest-pkg");
    assert_eq!(layout.package_root, dir);
    assert!(layout.manifest_path.ends_with("faber.toml"));
    // still sibling even with odd casing in name
    assert!(
        layout
            .debug_binary
            .to_string_lossy()
            .ends_with("manifest-pkg")
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn discover_build_layout_supports_directory_with_manifest() {
    let dir = test_temp_dir("layout-dir-manifest");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(dir.join("src/main.fab"), "main {}").expect("entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "dir-pkg"
"#,
    )
    .expect("manifest");

    let layout = discover_build_layout(&dir).expect("discover from dir");
    assert_eq!(layout.binary_name(), "dir-pkg");
    assert_eq!(
        layout.generated_rust_entry,
        dir.join("target/faber/src/main.rs")
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn direct_entry_file_under_manifest_uses_package_root_and_manifest_name() {
    let dir = test_temp_dir("layout-entry-parent-manifest");
    fs::create_dir_all(dir.join("src")).expect("src");
    let entry = dir.join("src/main.fab");
    fs::write(&entry, "main { print \"manifest root\" }").expect("entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "entry-parent"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");

    let spec = discover_package(&entry).expect("package discovery");
    assert_eq!(spec.source_root, dir.join("src"));
    assert_eq!(spec.entry, entry);

    let layout = discover_build_layout(&entry).expect("layout discovery");
    assert_eq!(layout.package_root, dir);
    assert_eq!(layout.binary_name(), "entry-parent");
    assert_eq!(
        layout.generated_rust_entry,
        layout.package_root.join("target/faber/src/main.rs")
    );
    assert!(
        !layout
            .generated_rust_entry
            .to_string_lossy()
            .contains("/src/target/faber/")
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn direct_non_manifest_entry_under_manifest_keeps_explicit_entry() {
    let dir = test_temp_dir("layout-explicit-entry-parent-manifest");
    fs::create_dir_all(dir.join("src")).expect("src");
    let manifest_entry = dir.join("src/main.fab");
    let explicit_entry = dir.join("src/other.fab");
    fs::write(&manifest_entry, "main { print \"manifest entry\" }").expect("manifest entry");
    fs::write(&explicit_entry, "main { print \"explicit entry\" }").expect("explicit entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "entry-parent"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");

    let spec = discover_package(&explicit_entry).expect("package discovery");
    assert_eq!(spec.source_root, dir.join("src"));
    assert_eq!(spec.entry, explicit_entry);

    let output = compile_with_faber_seams(&Config::default(), &explicit_entry)
        .output
        .expect("compile explicit entry");
    let rust = match output {
        radix::module::Output::Rust(output) => output.code,
        _ => panic!("expected Rust output"),
    };
    assert!(
        rust.contains("explicit entry"),
        "compiled wrong entry:\n{rust}"
    );
    assert!(
        !rust.contains("manifest entry"),
        "compiled manifest entry instead:\n{rust}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn discover_build_layout_supports_entry_file_input_and_falls_back_to_dir_name() {
    let dir = test_temp_dir("layout-entry-no-manifest");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"x\" }").expect("entry");

    let layout = discover_build_layout(&entry).expect("discover from entry file");
    // falls back to directory name since no manifest
    let expected_name = dir.file_name().unwrap().to_string_lossy().to_string();
    assert_eq!(layout.binary_name(), sanitize_crate_name(&expected_name));
    assert!(layout.cargo_target_dir.ends_with("target"));
}

#[test]
fn build_layout_never_produces_faber_target_nested_path() {
    let layout = BuildLayout::from_package_root("/tmp/xyz", "xyz");
    let nested = layout.generated_crate_root.join("target");
    assert!(
        !layout.debug_binary.starts_with(&nested),
        "no target/faber/target path allowed"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn linked_library_emit_skips_entry_file_layouts_without_manifest() {
    let dir = test_temp_dir("linked-library-no-manifest");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"no manifest\" }").expect("entry");

    let deps = radix::program::artifact_plan::native_library_deps(&dir)
        .expect("entry-file layouts without manifest should not require native deps");
    assert!(deps.is_empty());

    let layout = discover_build_layout(&entry).expect("layout");
    let linked = super::library_link::emit_linked_library_crates(&layout.package_root, &layout)
        .expect("entry-file layouts without manifest should not require native deps");

    assert_eq!(linked, [] as [super::library_link::LinkedLibraryCrate; 0]);
}

// ---------------------------------------------------------------------------
// Phase 2: Generated crate writer tests (no Cargo invocation)
// ---------------------------------------------------------------------------

#[cfg(feature = "hir-rust")]
#[test]
fn emit_generated_crate_writes_cargo_toml_and_main_rs_under_target_faber() {
    // The runtime is a path dependency on the named repository root.
    let pkg = test_temp_dir("emit-writer");
    let support_root = pkg.join("support-root");
    super::runtime_sources::set_path_override_for_test(Some(support_root.clone()));
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(pkg.join("src/main.fab"), r#"main { print "writer test" }"#).expect("entry");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "emit-test"
version = "0.3.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");

    let layout = discover_build_layout(&pkg).expect("layout");
    let compile_result = compile_with_faber_seams(&Config::default(), &pkg);
    assert!(compile_result.success(), "compile should succeed");
    let code = match &compile_result.output {
        Some(radix::module::Output::Rust(r)) => r.code.clone(),
        _ => panic!("expected rust output"),
    };

    let written = emit_generated_crate(
        &layout,
        &code,
        Some(&read_manifest(&layout.manifest_path).unwrap()),
    )
    .expect("emit");

    assert_eq!(written, layout.generated_crate_root);
    assert!(layout.generated_cargo_manifest.exists());
    assert!(layout.generated_rust_entry.exists());

    let cargo_toml = fs::read_to_string(&layout.generated_cargo_manifest).expect("read cargo");
    assert!(cargo_toml.contains("name = \"emit-test\""));
    assert!(cargo_toml.contains("edition = \"2021\""));
    assert!(cargo_toml.contains("0.3.0"));
    assert!(cargo_toml.contains("[dependencies]"));
    assert!(cargo_toml.contains("package = \"faber\""));
    assert!(cargo_toml.contains(&support_root.join("runtime/rust").display().to_string()));
    assert!(!cargo_toml.contains("git = "));
    assert!(!cargo_toml.contains("norma = { path = "));
    assert!(!cargo_toml.contains("tokio = { version = "));

    let main_rs = fs::read_to_string(&layout.generated_rust_entry).expect("read main");
    assert!(main_rs.contains("Generated by faber build"));
    assert!(main_rs.contains("writer test")); // from the source string

    // No nested target created by the writer
    assert!(!layout.generated_crate_root.join("target").exists());
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_runtime_plan_keeps_runtime_only_routes_hostless() {
    let pkg = test_temp_dir("runtime-plan-runtime-only");
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "runtime-plan-runtime-only"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        pkg.join("src/main.fab"),
        r#"main { let string body ← call 'runtime:echo' ("ok") ↦ string print body }"#,
    )
    .expect("entry");

    let plan = runtime_plan_with_faber_seams(&Config::default(), &pkg).expect("runtime plan");

    assert!(plan.non_runtime_routes.is_empty());
    assert!(plan.host.is_none());
    assert!(!plan.needs_tokio);
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_runtime_plan_inline_ad_leaf_needs_no_host() {
    // core-norma N2: solum:lege is the shared fs leaf. Inline ad without
    // [target.rust] host must plan; LeafDispatch answers at runtime.
    let pkg = test_temp_dir("runtime-plan-builtin-hostless");
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "runtime-plan-builtin-hostless"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        pkg.join("src/main.fab"),
        r#"main { let string body ← call 'solum:lege' ("data.txt") ↦ string print body }"#,
    )
    .expect("entry");

    let plan = runtime_plan_with_faber_seams(&Config::default(), &pkg).expect("runtime plan");

    assert_eq!(
        plan.non_runtime_routes.iter().cloned().collect::<Vec<_>>(),
        vec!["solum:lege".to_owned()]
    );
    assert!(plan.host.is_none());
    assert!(
        package_host_selection_diagnostic(&plan, &pkg.join("faber.toml")).is_none(),
        "solum:* is the shared fs leaf, not a host-required route"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_runtime_plan_served_route_needs_no_host() {
    // A3/U4: a route served by a Faber `@ ad` handler is tier 1, so it is
    // never host-required — the D8.1 test seam serves `solum:lege` itself.
    let pkg = test_temp_dir("runtime-plan-served-route");
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "runtime-plan-served-route"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        pkg.join("src/main.fab"),
        "@ call 'solum:lege'\nfn lege(string via) → string {\n    return via\n}\n\nmain { let string body ← call 'solum:lege' (\"data.txt\") ↦ string print body }\n",
    )
    .expect("entry");

    let plan = runtime_plan_with_faber_seams(&Config::default(), &pkg).expect("runtime plan");

    assert!(
        plan.non_runtime_routes.is_empty(),
        "served route must not be host-required: {:?}",
        plan.non_runtime_routes
    );
    assert!(package_host_selection_diagnostic(&plan, &pkg.join("faber.toml")).is_none());
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_runtime_plan_requires_host_for_host_only_routes() {
    // Host-only (not builtin) routes still require [target.rust] host.
    let pkg = test_temp_dir("runtime-plan-host-required");
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "runtime-plan-host-required"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        pkg.join("src/main.fab"),
        r#"main { let string body ← call 'ignotum:route' ("x") ↦ string print body }"#,
    )
    .expect("entry");

    let plan = runtime_plan_with_faber_seams(&Config::default(), &pkg).expect("runtime plan");

    assert_eq!(
        plan.non_runtime_routes.iter().cloned().collect::<Vec<_>>(),
        vec!["ignotum:route".to_owned()]
    );
    assert!(plan.host.is_none());
    let diagnostic = package_host_selection_diagnostic(&plan, &pkg.join("faber.toml"))
        .expect("missing host selection diagnostic");
    assert!(diagnostic_has_issue(
        &diagnostic,
        "package_host_selection_required"
    ));
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_runtime_plan_collects_library_ad_routes_without_host() {
    // Dual-backend honesty (S1-U3): norma-wrapped ad must appear in
    // non_runtime_routes. tempus:dormiet is the shared clock leaf, so a
    // hostless package using it plans without a host provider.
    let pkg = test_temp_dir("runtime-plan-library-tempus");
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "runtime-plan-library-tempus"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        pkg.join("src/main.fab"),
        r#"
import from "norma:time" time

main {
  time.sleep(0)
}
"#,
    )
    .expect("entry");

    let plan = runtime_plan_with_faber_seams(&Config::default(), &pkg).expect("runtime plan");

    assert!(
        plan.non_runtime_routes.contains("tempus:dormiet"),
        "library-expanded ad must be collected (got {:?})",
        plan.non_runtime_routes
    );
    assert!(plan.host.is_none());
    assert!(
        package_host_selection_diagnostic(&plan, &pkg.join("faber.toml")).is_none(),
        "tempus:* is the shared clock leaf, not a host-required route"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_runtime_plan_selects_faber_and_tokio_without_text_sniff() {
    // Phase 3: Cargo runtime deps must come from HIR/plan facts, never
    // `rust_code.contains("faber::")` / `contains("tokio::")`.
    let pkg = test_temp_dir("runtime-plan-no-text-sniff");
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "runtime-plan-no-text-sniff"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        pkg.join("src/main.fab"),
        r#"async_main { let string body ← call 'runtime:echo' ("ok") ↦ string print body }"#,
    )
    .expect("entry");

    let layout = discover_build_layout(&pkg).expect("layout");
    let package = analyze_with_faber_seams(&Config::default(), &pkg).expect("analyze");
    let artifact = radix::program::artifact_plan::plan_package(&package, Target::HirRust);
    assert!(
        artifact.has_runtime_dependency("rust:runtime:faber"),
        "artifact plan must list faber runtime"
    );
    assert!(
        artifact.has_runtime_dependency("rust:runtime:tokio"),
        "async entry must plan tokio runtime node"
    );

    let plan = runtime_plan_with_faber_seams(&Config::default(), &pkg).expect("runtime plan");
    assert!(plan.needs_faber);
    assert!(plan.needs_tokio);

    let compile_result = compile_with_faber_seams(&Config::default(), &pkg);
    assert!(compile_result.success(), "compile should succeed");
    let code = match &compile_result.output {
        Some(radix::module::Output::Rust(r)) => r.code.clone(),
        _ => panic!("expected rust output"),
    };
    // Emit uses the plan, not a text scan of `code`.
    emit_generated_crate_with_runtime_plan(
        &layout,
        &code,
        Some(&read_manifest(&layout.manifest_path).unwrap()),
        &plan,
    )
    .expect("emit");
    let cargo_toml = fs::read_to_string(&layout.generated_cargo_manifest).expect("read cargo");
    assert!(cargo_toml.contains("faber = { package = \"faber\""));
    assert!(cargo_toml.contains("tokio = { version = "));
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_runtime_plan_drives_tokio_dependency_without_source_scan() {
    let pkg = test_temp_dir("runtime-plan-async-entry");
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "runtime-plan-async-entry"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        pkg.join("src/main.fab"),
        r#"async_main { let string body ← call 'runtime:echo' ("ok") ↦ string print body }"#,
    )
    .expect("entry");

    let layout = discover_build_layout(&pkg).expect("layout");
    let compile_result = compile_with_faber_seams(&Config::default(), &pkg);
    assert!(compile_result.success(), "compile should succeed");
    let code = match &compile_result.output {
        Some(radix::module::Output::Rust(r)) => r.code.clone(),
        _ => panic!("expected rust output"),
    };
    let plan = runtime_plan_with_faber_seams(&Config::default(), &pkg).expect("runtime plan");
    assert!(plan.needs_tokio);

    emit_generated_crate_with_runtime_plan(
        &layout,
        &code,
        Some(&read_manifest(&layout.manifest_path).unwrap()),
        &plan,
    )
    .expect("emit");
    let cargo_toml = fs::read_to_string(&layout.generated_cargo_manifest).expect("read cargo");

    assert!(cargo_toml.contains("tokio = { version = "));
    assert!(!cargo_toml.contains("faber-host-macos-arm64"));
    assert!(!cargo_toml.contains("../radix/hosts/macos-arm64"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn emit_generated_crate_works_without_manifest_using_fallback_name() {
    let pkg = test_temp_dir("emit-no-manifest");
    let entry = pkg.join("main.fab");
    fs::write(&entry, "main {}").expect("entry");

    let layout = discover_build_layout(&entry).expect("layout");
    // Directly test the emit path with dummy code (no real compile needed for writer coverage)
    let dummy = "fn main(){}";
    let _ = emit_generated_crate(&layout, dummy, None).expect("emit fallback");

    let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo");
    assert!(cargo.contains(&format!("name = \"{}\"", layout.binary_name())));
}

#[cfg(feature = "hir-rust")]
#[test]
fn package_host_routes_fail_closed_with_and_without_native_host() {
    // The native host route and its providers were dropped (single-repo-
    // development Q3): a package that uses a host-required route
    // (`ignotum:route`) must fail at plan time whether or not
    // `[target.rust] host = "native"` is set, while the manifest line
    // itself still parses. `solum:*` is the shared fs leaf and is not
    // host-required (core-norma N2).
    for (label, target_rust, issue) in [
        (
            "package-host-route-hostless",
            "",
            "package_host_selection_required",
        ),
        (
            "package-host-route-native",
            "\n[target.rust]\nhost = \"native\"\n",
            "host_provider_unsupported",
        ),
    ] {
        let pkg = test_temp_dir(label);
        fs::create_dir_all(pkg.join("src")).expect("create src");
        fs::write(
            pkg.join("faber.toml"),
            format!(
                "[package]\nname = \"{label}\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n{target_rust}"
            ),
        )
        .expect("write manifest");
        fs::write(
            pkg.join("src/main.fab"),
            r#"main { let string body ← call 'ignotum:route' ("x") ↦ string print body }"#,
        )
        .expect("write entry");

        let plan = runtime_plan_with_faber_seams(&Config::default(), &pkg).expect("runtime plan");
        assert_eq!(
            plan.host.is_some(),
            !target_rust.is_empty(),
            "{label}: host policy must still parse"
        );
        let diagnostic = package_host_selection_diagnostic(&plan, &pkg.join("faber.toml"))
            .unwrap_or_else(|| panic!("{label}: host route must fail closed at plan time"));
        assert!(
            diagnostic_has_issue(&diagnostic, issue),
            "{label}: expected {issue}, got {diagnostic:?}"
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn generated_package_norma_json_facade_defers_runtime_codec() {
    let pkg = test_temp_dir("package-norma-json-formal-facade");
    let entry = pkg.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:json" json

main {
    do {
        let json duplicate ← json.parse("{\"id\": 1, \"id\": 2}")
        print duplicate
    }
    catch err {
        print "duplicate rejected"
    }

    do {
        let json unicode ← json.parse("{\"music\": \"\\uD834\\uDD1E\"}")
        print json.stringify(unicode)
    }
    catch err {
        print err
    }
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "compile should succeed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("pub mod json"));
    assert!(output.code.contains("fn parse"));
    assert!(output.code.contains("fn stringify"));
    assert!(output.code.contains("crate::json::solve::parse"));
    assert!(output.code.contains("crate::json::serialize::stringify"));
    assert!(output.code.contains("pub mod solve"));
    assert!(output.code.contains("pub mod serialize"));
    assert!(output.code.contains("fn parse_value"));
    assert!(output.code.contains("fn encode"));
    assert!(
        !output
            .code
            .contains("deferred pending runtime codec dispatch")
    );
    assert!(!output.code.contains("faber::Json::parse"));
    assert!(!output.code.contains(".to_wire()"));
    assert!(
        !output
            .code
            .contains("non-BMP \\\\u escapes not yet supported")
    );
    assert!(!output.code.contains("out.pone"));
}

// ---------------------------------------------------------------------------
// Transitive library imports (norma: closure + interface provenance)
// ---------------------------------------------------------------------------

#[cfg(feature = "hir-rust")]
fn repo_norma_source_file(relative_path: &str) -> PathBuf {
    radix::program::library_home()
        .expect("library home resolves in-tree")
        .join("norma/src")
        .join(format!("{relative_path}.fab"))
}

fn write_temp_library_fixture(dir: &Path, relative_path: &str, source: &str) {
    let path = dir.join("norma/src").join(format!("{relative_path}.fab"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create library fixture dir");
    }
    fs::write(path, source).expect("write library fixture");
}

#[cfg(feature = "hir-rust")]
fn seed_temp_library_text(library_home: &Path) {
    let source = repo_norma_source_file("text");
    let dest = library_home.join("norma/src/text.fab");
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).expect("create norma dir");
    }
    fs::copy(&source, &dest).expect("copy text.fab into temp library");
}

fn config_with_library_home(library_home: &Path) -> Config {
    Config::default().with_library_home(library_home.to_path_buf())
}

fn write_installed_library_manifest(
    library_home: &Path,
    provider: &str,
    source_root: &str,
    module_path: Option<&str>,
) {
    let package_root = library_home.join(provider);
    fs::create_dir_all(&package_root).expect("create installed package root");
    fs::write(
        package_root.join("faber.toml"),
        format!(
            r#"
[package]
name = "{provider}"
version = "0.1.0"

[library]
provider = "{provider}"

[paths]
source = "{source_root}"

[build]
kind = "lib"
targets = ["rust"]
"#
        ),
    )
    .expect("write installed manifest");
    if let Some(module_path) = module_path {
        let path = package_root
            .join(source_root)
            .join(format!("{module_path}.fab"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create installed source root");
        }
        fs::write(
            path,
            r#"
fn label() → string {
    return "installed"
}
"#,
        )
        .expect("write installed module");
    }
}

#[test]
fn library_resolver_reports_installed_manifest_missing_source_root() {
    let library_home = test_temp_dir("installed-missing-source-root-home");
    // A supported `src` root that was never materialized still surfaces the
    // missing-source-root diagnostic; an unsupported root is rejected even
    // earlier (see installed_library_with_unsupported_source_root_is_rejected).
    write_installed_library_manifest(library_home.path(), "altlib", "src", None);

    let dir = test_temp_dir("installed-missing-source-root-app");
    fs::create_dir_all(dir.join("src")).expect("create app src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "installed-missing-source-root-app"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write app manifest");
    fs::write(
        dir.join("src/main.fab"),
        r#"
import from "altlib:math/add" add

main {
    print add.label()
}
"#,
    )
    .expect("write app entry");

    let result = compile_with_faber_seams(&config_with_library_home(library_home.path()), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "missing_installed_library_source_root")
            && diagnostic_has_arg(diag, "provider", "altlib")
    }));
}

#[test]
fn installed_library_with_unsupported_source_root_is_rejected() {
    let library_home = test_temp_dir("installed-unsupported-source-root-home");
    // Stage 0 restricts `[paths].source` to `src` and `.`, so an installed
    // library whose manifest declares a depth-1 custom root is rejected at
    // discovery instead of silently resolving (or dropping) a source dir.
    write_installed_library_manifest(library_home.path(), "altlib", "interfaces", None);

    let dir = test_temp_dir("installed-unsupported-source-root-app");
    fs::create_dir_all(dir.join("src")).expect("create app src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "installed-unsupported-source-root-app"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write app manifest");
    fs::write(
        dir.join("src/main.fab"),
        r#"
import from "altlib:math/add" add

main {
    print add.label()
}
"#,
    )
    .expect("write app entry");

    let result = compile_with_faber_seams(&config_with_library_home(library_home.path()), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "invalid_installed_library_manifest")
            && diagnostic_has_arg(diag, "provider", "altlib")
            && diagnostic_has_arg(
                diag,
                "cause_issue",
                "package_member_unsupported_source_root",
            )
    }));
}

#[cfg(feature = "hir-rust")]
fn transitive_test_diagnostic_facts(
    result: &radix::module::CompileResult,
) -> Vec<(Option<&str>, Option<&str>)> {
    result
        .diagnostics
        .iter()
        .map(|diag| (diag.code, diag.issue()))
        .collect()
}

#[cfg(feature = "hir-rust")]
#[test]
fn transitive_nested_lista_lista_textus_typechecks() {
    let library_home = test_temp_dir("transitive-nested-lista-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "nested_lista_probe",
        r#"+++
locale = "la"
+++
@ publica
functio vacua_tabula() → lista<lista<textus>> {
    redde vacua
}
"#,
    );

    let dir = test_temp_dir("transitive-nested-lista-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:nested_lista_probe" nested_lista_probe

main {
    let list<list<string>> grid ← nested_lista_probe.vacua_tabula()
    print grid.length()
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&config_with_library_home(library_home.path()), &entry);
    assert!(
        result.success(),
        "expected nested lista<lista<textus>> typecheck success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn transitive_import_text_via_fixture_library_compiles() {
    let library_home = test_temp_dir("transitive-chorda-library_home");
    seed_temp_library_text(library_home.path());
    write_temp_library_fixture(
        library_home.path(),
        "transitive_fixture",
        r#"+++
locale = "la"
+++
importa ex "norma:text" chorda

@ publica
functio join_fields(lista<textus> parts) → textus {
    redde chorda.join(parts, ",")
}
"#,
    );

    let dir = test_temp_dir("transitive-chorda-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:transitive_fixture" transitive_fixture

main {
    let string wire ← transitive_fixture.join_fields(["alpha", "beta"])
    print wire
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&config_with_library_home(library_home.path()), &entry);
    assert!(
        result.success(),
        "expected transitive chorda fixture compile success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );

    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("pub mod text"));
    assert!(output.code.contains("crate::text::join"));
    assert!(output.code.contains("pub mod transitive_fixture"));
    assert!(!output.code.contains("unresolved_def"));
}

#[test]
fn transitive_library_import_cycle_is_rejected() {
    let library_home = test_temp_dir("transitive-cycle-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "cycle_a",
        r#"+++
locale = "la"
+++
importa ex "norma:cycle_b" cycle_b

functio label() → textus {
    redde "a"
}
"#,
    );
    write_temp_library_fixture(
        library_home.path(),
        "cycle_b",
        r#"+++
locale = "la"
+++
importa ex "norma:cycle_a" cycle_a

functio label() → textus {
    redde "b"
}
"#,
    );

    let dir = test_temp_dir("transitive-cycle-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:cycle_a" cycle_a

main {
    print cycle_a.label()
}
"#,
    )
    .expect("write entry");

    let config = config_with_library_home(library_home.path());
    let spec = discover_package(&entry).expect("package");
    let resolver = library_resolver_from_config(&config);
    let files = load_with_faber_seams(&spec, &resolver, Some(&english_pack()), false, None);
    assert!(files.is_err(), "cycle should fail package load");
    let err = files.err().expect("diagnostics");
    assert!(err.iter().any(|diag| {
        diagnostic_has_issue(diag, "library_import_cycle")
            && diagnostic_has_arg(diag, "cycle", "cycle_a -> cycle_b -> cycle_a")
    }));

    let result = compile_with_faber_seams(&config, &entry);
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "library_import_cycle"))
    );
}

#[test]
fn transitive_library_conflicting_aliases_are_rejected() {
    let dir = test_temp_dir("transitive-alias-conflict");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:csv" csv
import from "norma:text" text as ropes

main {
    print csv.parse("a,b").longitudo()
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "library_conflicting_aliases")
            && diagnostic_has_arg(diag, "module", "text")
            && diagnostic_has_arg(diag, "alias", "ropes")
    }));
}

#[cfg(feature = "hir-rust")]
#[test]
fn transitive_library_diamond_dedupes_shared_dependency() {
    let library_home = test_temp_dir("transitive-diamond-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "shared_d",
        r#"+++
locale = "la"
+++
@ publica
functio tag() → textus {
    redde "d"
}
"#,
    );
    write_temp_library_fixture(
        library_home.path(),
        "dep_b",
        r#"+++
locale = "la"
+++
importa ex "norma:shared_d" shared_d

@ publica
functio via_b() → textus {
    redde shared_d.tag()
}
"#,
    );
    write_temp_library_fixture(
        library_home.path(),
        "dep_c",
        r#"+++
locale = "la"
+++
importa ex "norma:shared_d" shared_d

@ publica
functio via_c() → textus {
    redde shared_d.tag()
}
"#,
    );
    write_temp_library_fixture(
        library_home.path(),
        "leaf_a",
        r#"+++
locale = "la"
+++
importa ex "norma:dep_b" dep_b
importa ex "norma:dep_c" dep_c

@ publica
functio both() → textus {
    redde dep_b.via_b() + dep_c.via_c()
}
"#,
    );

    let dir = test_temp_dir("transitive-diamond-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:leaf_a" leaf_a

main {
    print leaf_a.both()
}
"#,
    )
    .expect("write entry");

    let config = config_with_library_home(library_home.path());
    let spec = discover_package(&entry).expect("package");
    let resolver = library_resolver_from_config(&config);
    let files = load_with_faber_seams(&spec, &resolver, Some(&english_pack()), false, None)
        .expect("load package");
    let file = files
        .iter()
        .find(|file| file.path == entry)
        .expect("entry file");
    let shared_d_count = file
        .expanded_library_imports
        .iter()
        .filter(|import| import.binding == "shared_d")
        .count();
    assert_eq!(shared_d_count, 1, "shared_d should appear once in closure");

    let package = analyze_with_faber_seams(&config, &entry).expect("analyze package");
    let analysis = &package.entry_node().expect("entry unit").analysis;
    let leaf_a = library_binding_by_module(analysis, &["leaf_a"]);
    assert_eq!(leaf_a.identity.module_path, vec!["leaf_a"]);

    let result = compile_with_faber_seams(&config, &entry);
    assert!(
        result.success(),
        "expected diamond package compile success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );
}

#[test]
fn transitive_library_import_closure_preserves_publica_visibility() {
    let library_home = test_temp_dir("transitive-publica-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "child",
        r#"+++
locale = "la"
+++
functio label() → textus {
    redde "child"
}
"#,
    );
    write_temp_library_fixture(
        library_home.path(),
        "parent",
        r#"+++
locale = "la"
+++
importa ex "norma:child" publica child

functio parent_label() → textus {
    redde "parent"
}
"#,
    );

    let dir = test_temp_dir("transitive-publica-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:parent" parent

main {
    print parent.label()
}
"#,
    )
    .expect("write entry");

    let config = config_with_library_home(library_home.path());
    let spec = discover_package(&entry).expect("package");
    let resolver = library_resolver_from_config(&config);
    let files = load_with_faber_seams(&spec, &resolver, Some(&english_pack()), false, None)
        .expect("load package");
    let file = files
        .iter()
        .find(|file| file.path == entry)
        .expect("entry file");

    let child = file
        .expanded_library_imports
        .iter()
        .find(|import| import.binding == "child")
        .expect("child import");
    let parent = file
        .expanded_library_imports
        .iter()
        .find(|import| import.binding == "parent")
        .expect("parent import");
    assert_eq!(child.visibility, radix::module::syntax::Visibility::Publica);
    assert_eq!(
        parent.visibility,
        radix::module::syntax::Visibility::Privata
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn publica_library_import_surfaces_as_nested_namespace_binding() {
    let library_home = test_temp_dir("publica-reexport-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "child",
        r#"+++
locale = "la"
+++
@ publica
functio label() → textus {
    redde "child"
}
"#,
    );
    write_temp_library_fixture(
        library_home.path(),
        "parent",
        r#"+++
locale = "la"
+++
importa ex "norma:child" publica child

functio parent_label() → textus {
    redde "parent"
}
"#,
    );

    let dir = test_temp_dir("publica-reexport-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:parent" parent

main {
    print parent.child.label()
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&config_with_library_home(library_home.path()), &entry);
    assert!(
        result.success(),
        "expected public child re-export compile success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );

    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("crate::child::label()"));
    assert!(!output.code.contains("crate::parent::child::label()"));
}

#[test]
fn privata_transitive_library_import_does_not_surface_as_nested_namespace_binding() {
    let library_home = test_temp_dir("privata-transitive-library_home");
    write_temp_library_fixture(
        library_home.path(),
        "child",
        r#"+++
locale = "la"
+++
functio label() → textus {
    redde "child"
}
"#,
    );
    write_temp_library_fixture(
        library_home.path(),
        "parent",
        r#"+++
locale = "la"
+++
importa ex "norma:child" child

functio parent_label() → textus {
    redde "parent"
}
"#,
    );

    let dir = test_temp_dir("privata-transitive-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:parent" parent

main {
    print parent.child.label()
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&config_with_library_home(library_home.path()), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "namespace_missing_export")
            && diagnostic_has_arg(diag, "member", "child.label")
    }));
}

#[cfg(feature = "hir-rust")]
#[test]
fn transitive_csv_text_closure_compiles_with_mixed_provenance() {
    let dir = test_temp_dir("transitive-csv-chorda");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:csv" csv

main {
    let list<list<string>> grid ← csv.parse("name,count\nalpha,1")
    let string wire ← csv.stringify(grid)
    print wire
}
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let resolver = library_resolver_from_config(&config);
    let files = load_with_faber_seams(&spec, &resolver, Some(&english_pack()), false, None)
        .expect("load package");
    let file = files
        .iter()
        .find(|file| file.path == entry)
        .expect("entry file");
    assert_eq!(file.expanded_library_imports.len(), 2);

    let package = analyze_with_faber_seams(&config, &entry).expect("analyze package");
    let analysis = &package.entry_node().expect("entry unit").analysis;

    let csv_binding = library_binding_by_module(analysis, &["csv"]);
    assert_eq!(csv_binding.identity.module_path, vec!["csv"]);

    let result = compile_with_faber_seams(&config, &entry);
    assert!(
        result.success(),
        "expected norma:csv transitive compile success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );

    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("pub mod csv"));
    assert!(output.code.contains("pub mod text"));
    // N4a: split_n is an ordinary Faber body and must be emitted.
    assert!(output.code.contains("fn split_n"));
    assert!(output.code.contains("pub(crate) fn join"));

    let layout = discover_build_layout(&entry).expect("layout");
    emit_generated_crate(&layout, &output.code, None).expect("emit generated crate");
    let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo toml");
    assert!(!cargo.contains("norma = { path = "));
}

#[cfg(feature = "hir-rust")]
#[test]
fn transitive_csv_solve_empty_input_returns_vacua() {
    let dir = test_temp_dir("transitive-csv-empty");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:csv" csv

main {
    let list<list<string>> grid ← csv.parse("")
    print grid.length()
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected empty solve success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn transitive_csv_solve_preserves_trailing_row() {
    let dir = test_temp_dir("transitive-csv-trailing");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "norma:csv" csv

main {
    let list<list<string>> grid ← csv.parse("a,b\n")
    print grid.length()
}
"#,
    )
    .expect("write entry");

    let result = compile_with_faber_seams(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected trailing row solve success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );
}

#[test]
fn use_package_compiler_keeps_rust_and_metal_fab_on_package_path() {
    let fab = Path::new("main.fab");
    assert!(use_package_compiler(Target::HirRust, fab, false));
    assert!(use_package_compiler(Target::MirScena, fab, false));
    assert!(use_package_compiler(Target::MirFmir, fab, false));
    assert!(use_package_compiler(Target::MirFmirBinary, fab, false));
    assert!(use_package_compiler(Target::HirFhir, fab, false));
    assert!(use_package_compiler(Target::MirMetal, fab, false));
    assert!(!use_package_compiler(Target::MirWgsl, fab, false));
    assert!(!use_package_compiler(Target::MirLlvm, fab, false));
    assert!(!use_package_compiler(Target::HirTypeScript, fab, false));
    // A plain lone file (here: no such file on disk, so no import to declare)
    // keeps the single-file `clif` route; only an import-declaring file is
    // package-shaped for it.
    assert!(!use_package_compiler(Target::MirClif, fab, false));
}

#[cfg(feature = "mir-metal")]
const EMIT_METAL_KERNEL_MODULE: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn linear<size M, size K, size N>(tensor<f32, [M, K]> x, tensor<f32, [K, N]> w, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return (x · w) + b
}
"#;

#[cfg(feature = "mir-metal")]
const EMIT_METAL_HOST_CONSUMER: &str = r#"+++
locale = "en"
+++

import from "./nn" nn

fn main() → void {
    let _ x ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    let _ w ← [[1.0, 0.0], [0.0, 1.0]] ↦ tensor<f32, [2, 2]>
    let _ b ← [[0.1, 0.1], [0.1, 0.1]] ↦ tensor<f32, [2, 2]>
    let _ r ← nn.linear(x, w, b)
    print "ok"
}
"#;

/// SGR-U2: `emit -t metal-text` on a `.fab` entry takes the merged-consumer
/// lane — the same source the build branch compiles — instead of
/// `radix-program::emit`, whose artifact plan has no `MirMetal` arm and reports
/// the input as `package_target_unsupported`.
#[cfg(feature = "mir-metal")]
#[test]
fn emit_metal_text_package_entry_emits_imported_kernel_msl() {
    let dir = test_temp_dir("emit-metal-text");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(dir.join("src/nn.fab"), EMIT_METAL_KERNEL_MODULE).expect("kernel module");
    fs::write(dir.join("src/main.fab"), EMIT_METAL_HOST_CONSUMER).expect("host consumer");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "emit-metal-text"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("manifest");
    let entry = dir.join("src/main.fab");

    assert!(use_package_compiler(Target::MirMetal, &entry, false));
    let (config, _) = config_with_locale(Target::MirMetal, &entry, None, None).expect("config");
    let code = compile_package_metal_text(&config, &entry).expect("metal-text emit");
    assert!(
        code.contains("kernel void"),
        "expected emitted MSL kernels, got:\n{code}"
    );
}

/// Write a one-file package whose `main.fab` is `source`; return its entry and
/// the temp dir that must outlive it.
#[cfg(feature = "mir-metal")]
fn metal_kernel_package(name: &str, source: &str) -> (PathBuf, impl Sized) {
    let dir = test_temp_dir(name);
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(dir.join("src/main.fab"), source).expect("entry");
    fs::write(
        dir.join("faber.toml"),
        format!(
            r#"
[package]
name = "{name}"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#
        ),
    )
    .expect("manifest");
    (dir.join("src/main.fab"), dir)
}

/// `faber emit -t metal-text` on a package takes the package lane, which never
/// ran the per-target kernel requirement rows. A `u32` expression with a
/// 48-bit intermediate reached the Metal emitter and was multiplied in a
/// 32-bit `uint` (a silent wrap); the row's refusal now stops it by name.
#[cfg(feature = "mir-metal")]
#[test]
fn emit_metal_text_package_refuses_a_narrow_expression_with_a_wide_intermediate() {
    let (entry, _dir) = metal_kernel_package(
        "emit-metal-wide-intermediate",
        "+++\nlocale = \"en\"\n+++\n@ kernel
@ public
fn k_wide(tensor<u32, [8]> a, tensor<u32, [8]> b, mut tensor<u32, [8]> out) → void {
    for from a at [i] thread lane let x {
        out[[i]] ← ((x ∧ 16777215) * (b[[i]] ∧ 16777215)) / 1048576
    }
}
main {}
",
    );
    let (config, _) = config_with_locale(Target::MirMetal, &entry, None, None).expect("config");
    let diagnostics = compile_package_metal_text(&config, &entry)
        .expect_err("a narrow-typed wide intermediate must not emit");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("kernel_target_requirements_not_met")),
        "{diagnostics:?}"
    );
}

/// The refusal is the row's, not a blanket 64-bit ban: an `i64` kernel whose
/// intermediate is its own 64-bit type is admitted and emitted in `long`.
#[cfg(feature = "mir-metal")]
#[test]
fn emit_metal_text_package_still_emits_a_native_64_bit_kernel() {
    let (entry, _dir) = metal_kernel_package(
        "emit-metal-native-i64",
        "+++\nlocale = \"en\"\n+++\n@ kernel
@ public
fn k_sum(tensor<i64, [8]> a, tensor<i64, [8]> b, mut tensor<i64, [8]> out) → void {
    for from a at [i] thread lane let x {
        out[[i]] ← (x ∧ 4294967295) + (b[[i]] ∧ 4294967295)
    }
}
main {}
",
    );
    let (config, _) = config_with_locale(Target::MirMetal, &entry, None, None).expect("config");
    let code = compile_package_metal_text(&config, &entry).expect("an i64 kernel emits");
    assert!(code.contains("constant long* b0 [[buffer(0)]]"), "{code}");
}

/// The `clif` package build emits a native object through the package-MIR
/// object emitter. A leafless package is inside the CLIF capability subset, so
/// the object must arrive with bytes and no diagnostics.
#[cfg(feature = "mir-clif")]
#[test]
fn clif_package_object_emits_for_a_leafless_package() {
    let dir = test_temp_dir("clif-package-object");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(dir.join("src/main.fab"), "main {}\n").expect("entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "clif-package-object"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("manifest");
    let entry = dir.join("src/main.fab");

    assert!(use_package_compiler(Target::MirClif, dir.path(), false));
    let (config, _) = config_with_locale(Target::MirClif, &entry, None, None).expect("config");
    let result = compile_package_clif(&config, &entry, &super::mir::faber_mir_seams());
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let Some(radix::module::Output::ClifObject(object)) = result.output else {
        panic!("expected a clif object output");
    };
    assert!(!object.bytes.is_empty(), "expected emitted object bytes");
}

/// A manifestless `.fab` file that imports a sibling module is package-shaped
/// for `clif`: the single-file object carries no linked sibling code, so the
/// imported call declines as CODEGEN001 `external definition call`, while the
/// package-MIR emitter links the import graph and emits a runnable object.
/// Files without such imports keep the single-file route.
#[cfg(feature = "mir-clif")]
#[test]
fn clif_file_mode_links_import_declaring_fab_through_the_package_lane() {
    let dir = test_temp_dir("clif-file-mode-import");
    fs::write(
        dir.join("main.fab"),
        r#"import from "./auxilium" auxilium

main {
    print auxilium.saluta("Marcus")
}
"#,
    )
    .expect("entry");
    fs::write(
        dir.join("auxilium.fab"),
        "@ public\nfn saluta(string nomen) → string {\n    return \"Salve, §!\"(nomen)\n}\n\nmain {}\n",
    )
    .expect("sibling");
    fs::write(dir.join("plain.fab"), "main {\n    print 1 + 1\n}\n").expect("plain");

    let entry = dir.join("main.fab");
    let plain = dir.join("plain.fab");
    assert!(manifestless_file_declares_non_kernel_import(&entry));
    assert!(!manifestless_file_declares_non_kernel_import(&plain));
    assert!(use_package_compiler(Target::MirClif, &entry, false));
    assert!(!use_package_compiler(Target::MirClif, &plain, false));

    // The single-file route still declines the imported call by name: the
    // routing above is what links it.
    let single_file = radix::services::compile_cli_path(&entry, false, Target::MirClif);
    assert!(single_file.output.is_none());
    assert!(
        single_file.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == Some("CODEGEN001")
                && diagnostic_has_arg(diagnostic, "category", "missing-abi-fact")
                && diagnostic_has_arg(diagnostic, "shape", "external definition call")
        }),
        "{:?}",
        single_file.diagnostics
    );

    let result = compile_clif_package_input(&entry);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let Some(radix::module::Output::ClifObject(object)) = result.output else {
        panic!("expected a clif object output");
    };
    assert!(!object.bytes.is_empty(), "expected emitted object bytes");
}

#[test]
fn use_package_compiler_from_args_honors_force_package_for_probe_targets() {
    let input = vec!["main.fab".to_owned()];
    assert!(use_package_compiler_from_args(
        Target::MirWgsl,
        &input,
        true
    ));
}

#[test]
fn g4_artifact_plan_is_deterministic_for_analyzed_package() {
    let dir = test_temp_dir("g4-plan");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "g4-plan"
version = "0.1.0"

[paths]
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(dir.join("src/main.fab"), "main { }\n").expect("entry");
    let package = analyze_with_faber_seams(&Config::default(), &dir).expect("analyze");
    let a = radix::program::artifact_plan::plan_package(&package, Target::HirRust);
    let b = radix::program::artifact_plan::plan_package(&package, Target::HirRust);
    assert!(a.supported);
    assert_eq!(a.to_debug_json().unwrap(), b.to_debug_json().unwrap());
    assert!(a.nodes.iter().any(|n| n.id.starts_with("rust:entry:")));
}

#[cfg(feature = "hir-rust")]
#[test]
#[ignore = "native library link e2e builds generated Rust; run with ignored integration checks"]
fn g4_native_library_links_into_application_build() {
    let root = test_temp_dir("g4-lib-link");
    let lib = root.join("libmath");
    let app = root.join("app");
    fs::create_dir_all(lib.join("src")).expect("lib src");
    fs::create_dir_all(lib.join("bindings")).expect("bindings");
    fs::create_dir_all(lib.join("rust")).expect("rust");
    fs::create_dir_all(app.join("src")).expect("app src");

    fs::write(
        lib.join("faber.toml"),
        r#"[package]
name = "libmath"
version = "0.1.0"
edition = "2026"

[library]
provider = "libmath"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[target.rust]
bindings = "bindings/rust.toml"
"#,
    )
    .expect("lib manifest");
    fs::write(
        lib.join("src/math.fab"),
        "@ public\nfn double(int n) → int\n",
    )
    .expect("lib source");
    fs::write(
        lib.join("rust/shim.rs"),
        "pub fn double(n: i64) -> i64 { n * 2 }\n",
    )
    .expect("shim");
    fs::write(
        lib.join("bindings/rust.toml"),
        r#"[functions."libmath:math.double"]
symbol = "crate::shim::double"

[shim]
path = "rust/shim.rs"
"#,
    )
    .expect("bindings");

    // Interface root for lock/resolver: same as package source.
    let interface_root = lib.join("src");
    fs::write(
        app.join("faber.toml"),
        r#"[package]
name = "g4-app"
version = "0.1.0"

[paths]
entry = "main.fab"

[dependencies]
libmath = "0.1.0"
"#,
    )
    .expect("app manifest");
    fs::write(
        app.join("faber.lock"),
        format!(
            r#"
[[package]]
name = "libmath"
version = "0.1.0"
source = "path"
package_root = "{package_root}"
kind = "lib"
target_language = "rust"
target_triple = "host"
target_manifest = ""
interface_root = "{interface_root}"
artifact = ""
crate = "libmath"
rustc = ""
"#,
            package_root = lib.display(),
            interface_root = interface_root.display(),
        ),
    )
    .expect("lock");
    fs::write(
        app.join("src/main.fab"),
        r#"
import from "libmath:math" math

main {
  let int n ← math.double(21)
  print n
}
"#,
    )
    .expect("app entry");

    let report = verify_library_binding_shapes(&lib, "rust").expect("library verifies");
    assert_eq!(report.bindings, 1);

    let result = compile_with_faber_seams(&Config::default(), &app);
    assert!(
        result.success(),
        "expected app compile success, got {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    // Calls route through the linked crate, not an inlined panic body.
    assert!(
        output.code.contains("libmath::math::double") || output.code.contains("libmath::double"),
        "expected external library call path, got:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("reached Rust codegen without a body"),
        "bodyless library must not be inlined as panic stubs"
    );

    let layout = discover_build_layout(&app).expect("layout");
    let linked = super::library_link::emit_linked_library_crates(&app, &layout)
        .expect("emit library crates");
    assert_eq!(linked.len(), 1);
    assert!(linked[0].crate_root.join("src/lib.rs").is_file());
    assert!(linked[0].crate_root.join("Cargo.toml").is_file());

    let mut plan = runtime_plan_with_faber_seams(&Config::default(), &app).expect("runtime plan");
    plan.library_path_deps = linked
        .into_iter()
        .map(|l| (l.crate_name, l.crate_root))
        .collect();
    let meta = read_manifest(&layout.manifest_path).ok();
    emit_generated_crate_with_runtime_plan(&layout, &output.code, meta.as_ref(), &plan)
        .expect("emit app crate");
    let binary = invoke_cargo_build(&layout, false).expect("cargo build app with library dep");
    assert!(binary.is_file(), "binary missing at {}", binary.display());

    let run = Command::new(&binary).output().expect("run linked binary");
    assert!(
        run.status.success(),
        "run failed: stdout={} stderr={}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("42"),
        "expected doubled value 42, got {stdout:?}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn g4_relative_lock_paths_resolve_against_app_package_root() {
    // Lock package_root/interface_root may be relative to the app that owns
    // faber.lock — never to the process CWD.
    let root = test_temp_dir("g4-rel-lock");
    let lib = root.join("libmath");
    let app = root.join("app");
    fs::create_dir_all(lib.join("src")).expect("lib src");
    fs::create_dir_all(lib.join("bindings")).expect("bindings");
    fs::create_dir_all(lib.join("rust")).expect("rust");
    fs::create_dir_all(app.join("src")).expect("app src");

    fs::write(
        lib.join("faber.toml"),
        r#"[package]
name = "libmath"
version = "0.1.0"
edition = "2026"

[library]
provider = "libmath"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[target.rust]
bindings = "bindings/rust.toml"
"#,
    )
    .expect("lib manifest");
    fs::write(
        lib.join("src/math.fab"),
        "@ public\nfn double(int n) → int\n",
    )
    .expect("lib source");
    fs::write(
        lib.join("rust/shim.rs"),
        "pub fn double(n: i64) -> i64 { n * 2 }\n",
    )
    .expect("shim");
    fs::write(
        lib.join("bindings/rust.toml"),
        r#"[functions."libmath:math.double"]
symbol = "crate::shim::double"

[shim]
path = "rust/shim.rs"
"#,
    )
    .expect("bindings");

    fs::write(
        app.join("faber.toml"),
        r#"[package]
name = "g4-rel-app"
version = "0.1.0"

[paths]
entry = "main.fab"

[dependencies]
libmath = "0.1.0"
"#,
    )
    .expect("app manifest");
    fs::write(
        app.join("faber.lock"),
        r#"
[[package]]
name = "libmath"
version = "0.1.0"
source = "path"
package_root = "../libmath"
kind = "lib"
target_language = "rust"
target_triple = "host"
target_manifest = ""
interface_root = "../libmath/src"
artifact = ""
crate = "libmath"
rustc = ""
"#,
    )
    .expect("lock");
    fs::write(
        app.join("src/main.fab"),
        r#"
import from "libmath:math" math

main {
  let int n ← math.double(21)
  print n
}
"#,
    )
    .expect("app entry");

    // Call while CWD is not the app root — relative lock paths must still resolve.
    let old = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&root).expect("chdir root");
    // D1 offline seam: the linked library's faber runtime row falls back to
    // the container-local checkout behind the path override.
    let override_root = root.join("container");
    fs::create_dir_all(override_root.join("runtime/rust")).expect("override runtime");
    super::runtime_sources::set_path_override_for_test(Some(override_root));
    let deps = radix::program::artifact_plan::native_library_deps(&app).expect("native deps");
    assert_eq!(
        deps.len(),
        1,
        "relative package_root should resolve vs app root"
    );
    assert!(
        Path::new(&deps[0].1.package_root).is_absolute(),
        "emit path should be absolute after resolve"
    );
    let layout = discover_build_layout(&app).expect("layout");
    let linked = super::library_link::emit_linked_library_crates(&app, &layout).expect("emit");
    assert_eq!(linked.len(), 1);
    std::env::set_current_dir(old).expect("restore cwd");
}

#[test]
fn g4_package_target_rejects_unsupported_after_analysis() {
    let dir = test_temp_dir("g4-reject");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "g4-reject"
version = "0.1.0"

[paths]
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(dir.join("src/main.fab"), "main { }\n").expect("entry");
    // Wasm is now a supported package target (U6-D); wgsl stays a rejected
    // package target for the fail-closed seam.
    let result = compile_with_faber_seams(&Config::default().with_target(Target::MirWgsl), &dir);
    assert!(!result.success());
    assert!(result.diagnostics.iter().any(|d| diagnostic_has_issue(
        d,
        "package_target_unsupported"
    ) || diagnostic_has_issue(
        d,
        "package_target_assembly_pending"
    )));
}

#[cfg(feature = "hir-rust")]
#[cfg(feature = "hir-go")]
#[ignore = "requires a go toolchain on PATH; manual native build proof"]
#[test]
fn g6_go3_single_entry_cli_package_compiles_and_go_builds() {
    let dir = test_temp_dir("g6-go3-true");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "g6-go3-true"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    // SupportedNarrow: fixed exit 0, rest textus only, no options.
    fs::write(
        dir.join("src/main.fab"),
        r#"
@ cli "true"
@ operand rest string ignored
main args args exit 0 {
}
"#,
    )
    .expect("entry");

    let result = compile_with_faber_seams(&Config::default().with_target(Target::HirGo), &dir);
    assert!(
        result.success(),
        "go package compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::Go(output)) = result.output else {
        panic!("expected Go output");
    };
    assert!(
        output.code.contains("func main()") && output.code.contains("os.Exit(0)"),
        "expected Go CLI main with fixed exit:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("\n    0\n"),
        "fixed CLI exit must not emit a bare numeric statement:\n{}",
        output.code
    );

    let layout = discover_build_layout(&dir).expect("layout");
    let go_layout = super::GoBuildLayout::from_package(&layout);
    super::emit_go_module(&go_layout, &output.code, &[]).expect("emit go");
    let binary = super::invoke_go_build_default(&go_layout).expect("go build");
    assert!(binary.exists(), "binary missing at {}", binary.display());

    let status = Command::new(&binary)
        .args(["ignored", "args"])
        .status()
        .expect("run binary");
    assert_eq!(status.code(), Some(0), "true-style binary should exit 0");
}

#[cfg(feature = "hir-rust")]
#[cfg(feature = "hir-go")]
#[ignore = "requires a go toolchain on PATH; manual native build proof"]
#[test]
fn g6_go4_multi_module_namespace_package_builds() {
    let dir = test_temp_dir("g6-go4-multi");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "g6-go4-multi"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        dir.join("src/helper.fab"),
        "@ public\nfn identity(string s) → string {\n  return s\n}\n",
    )
    .expect("helper");
    fs::write(
        dir.join("src/main.fab"),
        r#"
import from "./helper" helper

@ cli "tool"
@ operand rest string ignored
main args args exit 0 {
  let string _ ← helper.identity("x")
}
"#,
    )
    .expect("entry");

    let result = compile_go_with_faber_seams(&Config::default().with_target(Target::HirGo), &dir);
    let modules = result.go_modules;
    let result = result.compile_result;
    assert!(
        result.success(),
        "go multi-module compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::Go(output)) = result.output else {
        panic!("expected Go output");
    };
    assert!(
        output.code.contains("var helper = struct") || output.code.contains("var helper ="),
        "expected namespace var for helper:\n{}",
        output.code
    );
    assert!(
        output.code.contains("Identity:") || output.code.contains("Identity "),
        "expected capitalized Identity field:\n{}",
        output.code
    );

    assert!(!modules.is_empty(), "expected non-entry module files");
    assert!(
        modules
            .iter()
            .any(|(_, body)| body.contains("func identity")),
        "expected identity func in module: {modules:?}"
    );

    let layout = discover_build_layout(&dir).expect("layout");
    let go_layout = super::GoBuildLayout::from_package(&layout);
    super::emit_go_module(&go_layout, &output.code, &modules).expect("emit go");
    let binary = super::invoke_go_build_default(&go_layout).expect("go build");
    let status = Command::new(&binary).status().expect("run");
    assert_eq!(status.code(), Some(0));
}

#[cfg(feature = "hir-go")]
#[test]
fn g6_go4_func_name_collision_fails_closed() {
    let dir = test_temp_dir("g6-go4-collision");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "g6-go4-collision"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    // Two non-entry modules export the same function name.
    fs::write(
        dir.join("src/a.fab"),
        "@ public\nfn identity(string s) → string {\n  return s\n}\n",
    )
    .expect("a");
    fs::write(
        dir.join("src/b.fab"),
        "@ public\nfn identity(string s) → string {\n  return s + \"!\"\n}\n",
    )
    .expect("b");
    fs::write(
        dir.join("src/main.fab"),
        r#"
import from "./a" a
import from "./b" b
@ cli "tool"
@ operand rest string ignored
main args args exit 0 {
  let string _ ← a.identity("x")
  let string __ ← b.identity("y")
}
"#,
    )
    .expect("entry");

    let result = compile_with_faber_seams(&Config::default().with_target(Target::HirGo), &dir);
    assert!(!result.success(), "expected compile failure on collision");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| diagnostic_has_issue(d, "package_go_func_name_collision")),
        "expected package_go_func_name_collision: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-go")]
fn write_go_module_package(dir: &Path, pkg_name: &str, func_name: &str) {
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        format!(
            r#"
[package]
name = "{pkg_name}"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#
        ),
    )
    .expect("manifest");
    fs::write(
        dir.join("src/helper.fab"),
        format!("@ public\nfn {func_name}(string s) → string {{\n  return s\n}}\n"),
    )
    .expect("helper");
    fs::write(
        dir.join("src/main.fab"),
        format!(
            r#"
import from "./helper" helper

@ cli "tool"
@ operand rest string ignored
main args args exit 0 {{
  let string _ ← helper.{func_name}("x")
}}
"#
        ),
    )
    .expect("entry");
}

#[cfg(feature = "hir-go")]
#[test]
fn go_compile_failed_call_carries_no_modules_and_leaves_no_side_channel() {
    // Two non-entry modules exporting the same function name fail closed at
    // package assembly (see g6_go4_func_name_collision_fails_closed).
    let bad = test_temp_dir("go-fbr-collision");
    fs::create_dir_all(bad.join("src")).expect("src");
    fs::write(
        bad.join("faber.toml"),
        r#"
[package]
name = "go-fbr-collision"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        bad.join("src/a.fab"),
        "@ public\nfn identity(string s) → string {\n  return s\n}\n",
    )
    .expect("a");
    fs::write(
        bad.join("src/b.fab"),
        "@ public\nfn identity(string s) → string {\n  return s + \"!\"\n}\n",
    )
    .expect("b");
    fs::write(
        bad.join("src/main.fab"),
        r#"
import from "./a" a
import from "./b" b
@ cli "tool"
@ operand rest string ignored
main args args exit 0 {
  let string _ ← a.identity("x")
  let string __ ← b.identity("y")
}
"#,
    )
    .expect("entry");

    let failed = compile_go_with_faber_seams(&Config::default().with_target(Target::HirGo), &bad);
    assert!(
        !failed.compile_result.success(),
        "expected Go compile failure on name collision"
    );
    assert!(
        failed.go_modules.is_empty(),
        "failed Go compile must carry no modules"
    );

    // A later successful compile must produce exactly its own module set.
    let good = test_temp_dir("go-fbr-good");
    write_go_module_package(&good, "go-fbr-good", "identity");
    let ok = compile_go_with_faber_seams(&Config::default().with_target(Target::HirGo), &good);
    assert!(ok.compile_result.success(), "good package must compile");
    assert_eq!(ok.go_modules.len(), 1, "expected one sibling module");
    assert!(
        ok.go_modules[0].1.contains("func identity"),
        "expected identity func in module: {:?}",
        ok.go_modules
    );
}

#[cfg(feature = "hir-ts")]
#[test]
fn ts_package_emit_folds_sibling_module_into_one_stream() {
    // regio shape (corpus-product/regio): the entry imports `./planum` with no
    // binding, the sibling declares `module geometry`, and the fold must drop
    // the sibling import because the unit body now lives in the same stream.
    let dir = test_temp_dir("ts-fold");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "ts-fold"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        dir.join("src/planum.fab"),
        "module geometry\n\n@ public\nfn area(int width, int height) → int {\n  return width * height\n}\n",
    )
    .expect("planum");
    fs::write(
        dir.join("src/main.fab"),
        "import from \"./planum\"\n\nmain {\n  print geometry.area(3, 4)\n}\n",
    )
    .expect("entry");

    let result =
        compile_with_faber_seams(&Config::default().with_target(Target::HirTypeScript), &dir);
    assert!(
        result.success(),
        "expected TS package fold to succeed: {:?}",
        result.diagnostics
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| diagnostic_has_issue(d, "package_target_assembly_pending")),
        "TS fold must not report a pending assembly: {:?}",
        result.diagnostics
    );
    let Some(Output::TypeScript(output)) = result.output else {
        panic!("expected TypeScript output");
    };
    assert!(
        output
            .code
            .lines()
            .any(|line| line.contains("function area")),
        "folded stream must declare area: {}",
        output.code
    );
    assert!(
        !output
            .code
            .lines()
            .any(|line| line.contains("from \"./planum\"")),
        "folded stream must not re-import the sibling: {}",
        output.code
    );
}

#[cfg(feature = "hir-ts")]
#[test]
fn ts_package_emit_reports_merge_collision() {
    // Two non-entry units export the same top-level function name; folding them
    // into one stream must fail closed with the named merge diagnostic.
    let dir = test_temp_dir("ts-merge-collision");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "ts-merge-collision"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        dir.join("src/a.fab"),
        "@ public\nfn identity(string s) → string {\n  return s\n}\n",
    )
    .expect("a");
    fs::write(
        dir.join("src/b.fab"),
        "@ public\nfn identity(string s) → string {\n  return s + \"!\"\n}\n",
    )
    .expect("b");
    fs::write(
        dir.join("src/main.fab"),
        r#"
import from "./a" a
import from "./b" b
@ cli "tool"
@ operand rest string ignored
main args args exit 0 {
  let string _ ← a.identity("x")
  let string __ ← b.identity("y")
}
"#,
    )
    .expect("entry");

    let result =
        compile_with_faber_seams(&Config::default().with_target(Target::HirTypeScript), &dir);
    assert!(!result.success(), "expected TS merge collision failure");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| diagnostic_has_issue(d, "package_ts_merge_collision")),
        "expected package_ts_merge_collision diagnostic: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-go")]
#[test]
fn go_compile_repeated_and_nested_calls_do_not_exchange_modules() {
    let a = test_temp_dir("go-fbr-a");
    write_go_module_package(&a, "go-fbr-a", "alpha");
    let b = test_temp_dir("go-fbr-b");
    write_go_module_package(&b, "go-fbr-b", "beta");

    let first = compile_go_with_faber_seams(&Config::default().with_target(Target::HirGo), &a);
    assert!(first.compile_result.success(), "package a must compile");
    assert_eq!(first.go_modules.len(), 1, "package a has one sibling");
    assert!(
        first.go_modules[0].1.contains("func alpha"),
        "package a module must carry alpha: {:?}",
        first.go_modules
    );

    // Repeated call for a different package must not clobber the first
    // result's modules.
    let second = compile_go_with_faber_seams(&Config::default().with_target(Target::HirGo), &b);
    assert!(second.compile_result.success(), "package b must compile");
    assert_eq!(second.go_modules.len(), 1, "package b has one sibling");
    assert!(
        second.go_modules[0].1.contains("func beta"),
        "package b module must carry beta: {:?}",
        second.go_modules
    );
    assert!(
        first.go_modules[0].1.contains("func alpha")
            && !first.go_modules[0].1.contains("func beta"),
        "first result must keep its own modules: {:?}",
        first.go_modules
    );

    // Nested: a compile that runs while earlier results are still held keeps
    // its own modules and leaves the earlier results untouched.
    let third = compile_go_with_faber_seams(&Config::default().with_target(Target::HirGo), &a);
    assert!(
        third.compile_result.success(),
        "repeat of package a compiles"
    );
    assert_eq!(third.go_modules.len(), 1);
    assert!(
        third.go_modules[0].1.contains("func alpha")
            && !third.go_modules[0].1.contains("func beta"),
        "nested result must carry only its own modules: {:?}",
        third.go_modules
    );
    assert!(
        second.go_modules[0].1.contains("func beta"),
        "held result must be untouched by the nested compile: {:?}",
        second.go_modules
    );
}

#[cfg(feature = "hir-go")]
#[test]
fn go_compile_modules_stay_scoped_across_non_go_compiles() {
    let go_dir = test_temp_dir("go-fbr-scope");
    write_go_module_package(&go_dir, "go-fbr-scope", "alpha");
    let go_result =
        compile_go_with_faber_seams(&Config::default().with_target(Target::HirGo), &go_dir);
    assert!(go_result.compile_result.success());
    assert!(
        !go_result.go_modules.is_empty(),
        "go result must carry its sibling modules"
    );

    // A Rust package compile in between must not disturb the held Go result
    // and must not gain any Go modules itself.
    let rust_dir = test_temp_dir("go-fbr-rust");
    fs::create_dir_all(rust_dir.join("src")).expect("src");
    fs::write(
        rust_dir.join("faber.toml"),
        r#"
[package]
name = "go-fbr-rust"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(rust_dir.join("src/main.fab"), "main { }\n").expect("entry");
    let rust_result = compile_with_faber_seams(&Config::default(), &rust_dir);
    assert!(rust_result.success(), "rust package must compile");
    assert!(
        matches!(rust_result.output, Some(Output::Rust(_))),
        "expected Rust output for the non-Go compile"
    );

    assert!(
        go_result
            .go_modules
            .iter()
            .any(|(_, body)| body.contains("func alpha")),
        "go result must keep its own modules after a non-Go compile"
    );
}

#[cfg(feature = "hir-go")]
#[test]
fn go_compile_concurrent_calls_do_not_exchange_modules() {
    let a = test_temp_dir("go-fbr-conc-a");
    write_go_module_package(&a, "go-fbr-conc-a", "alpha");
    let b = test_temp_dir("go-fbr-conc-b");
    write_go_module_package(&b, "go-fbr-conc-b", "beta");

    let a_path = a.to_path_buf();
    let b_path = b.to_path_buf();
    let handle_a = thread::spawn(move || {
        compile_go_with_faber_seams(&Config::default().with_target(Target::HirGo), &a_path)
    });
    let handle_b = thread::spawn(move || {
        compile_go_with_faber_seams(&Config::default().with_target(Target::HirGo), &b_path)
    });
    let result_a = handle_a.join().expect("thread a");
    let result_b = handle_b.join().expect("thread b");

    assert!(
        result_a.compile_result.success(),
        "concurrent a must compile"
    );
    assert!(
        result_b.compile_result.success(),
        "concurrent b must compile"
    );
    assert!(
        result_a
            .go_modules
            .iter()
            .all(|(_, body)| body.contains("func alpha")),
        "concurrent a must carry only its own modules: {:?}",
        result_a.go_modules
    );
    assert!(
        result_b
            .go_modules
            .iter()
            .all(|(_, body)| body.contains("func beta")),
        "concurrent b must carry only its own modules: {:?}",
        result_b.go_modules
    );
    assert!(
        !result_a
            .go_modules
            .iter()
            .any(|(_, body)| body.contains("func beta"))
            && !result_b
                .go_modules
                .iter()
                .any(|(_, body)| body.contains("func alpha")),
        "concurrent compiles must not exchange module output"
    );
}

// ---------------------------------------------------------------------------
// G10 WEB1 — tela framework package contract
// ---------------------------------------------------------------------------

/// Lock a consumer app against a `tela` provider (web-mig U2):
/// the `WebController` annotation contract and the DOM `Scope` genus come
/// from `tela:web` / `tela:dom`.
fn write_tela_consumer_app(app: &Path, lib: &Path, entry_body: &str) {
    fs::create_dir_all(app.join("src")).expect("app src");
    let interface_root = lib.join("src");
    fs::write(
        app.join("faber.toml"),
        r#"[package]
name = "g10-tela-app"
version = "0.1.0"

[paths]
entry = "main.fab"

[reader]
locale = "en"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"

[dependencies]
tela = "0.1.0"
"#,
    )
    .expect("app manifest");
    fs::write(
        app.join("faber.lock"),
        format!(
            r#"
[[package]]
name = "tela"
version = "0.1.0"
source = "path"
package_root = "{package_root}"
kind = "lib"
target_language = "ts"
target_triple = "browser"
target_manifest = ""
interface_root = "{interface_root}"
artifact = ""
crate = "tela"
rustc = ""
"#,
            package_root = lib.display(),
            interface_root = interface_root.display(),
        ),
    )
    .expect("lock");
    fs::write(app.join("src/main.fab"), entry_body).expect("entry");
}

/// Write a minimal `tela` provider package: the `WebController` annotation
/// contract (`tela:web`) and the DOM surface (`tela:dom`) with a TS binding
/// shim. Consumer fixtures lock this package as `tela`, so origin verification
/// runs against the tela provider identity without a live tela or faber-web
/// checkout.
///
/// Public marks: `WebController`, `Scope`, `Element`, `FrameState`,
/// `SubmitOptions`, `scope`, `require`, `text_set`, `on_submit`, `on_frame`.
/// That is the post-U1 tela public set these fixtures need. Live tela in this
/// packet (pre-U1) marks only `Scope` + `WebController`; the extra marks here
/// are the documented delta, not a byte-mirror of the checkout.
fn write_synthetic_tela_lib(lib: &Path) {
    fs::create_dir_all(lib.join("src")).expect("lib src");
    fs::create_dir_all(lib.join("bindings")).expect("lib bindings");
    fs::create_dir_all(lib.join("runtime")).expect("lib runtime");
    fs::write(
        lib.join("faber.toml"),
        r#"[package]
name = "tela"
version = "0.1.0"

[library]
provider = "tela"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["ts"]

[target.ts]
bindings = "bindings/ts.toml"

[reader]
locale = "en"
"#,
    )
    .expect("tela manifest");
    fs::write(
        lib.join("src/web.fab"),
        r#"+++
locale = "en"
+++

# tela:web fixture — the WebController annotation contract (en surface,
# mirroring tela/src/web.fab's shape for the tela provider).

@ public
@ annotation { target = fn }
class WebController {
    mut string selector
}
"#,
    )
    .expect("web.fab");
    fs::write(
        lib.join("runtime/web.ts"),
        "export function webNoop(): void {}\n",
    )
    .expect("web shim");
    fs::write(
        lib.join("src/dom.fab"),
        r#"+++
locale = "en"
+++

# tela:dom fixture — the DOM surface (en), mirroring tela/src/dom.fab's
# shape for the symbols these product tests import.

@ public
class Scope {
    mut string selector = ""
}

@ public
class Element {
    mut string selector = ""
}

@ public
class FrameState {
    mut int frame = 0
    mut f32 time_ms = 0.0
    mut f32 delta_ms = 0.0
}

@ public
class SubmitOptions {
    mut bool prevent_default = true
}

class Subscription {
    mut int id = 0
}

type SubmitHandler = (Element) → void

type FrameHandler = (FrameState) → void

@ public
fn scope(string selector) → Scope {
    return Scope { selector = selector }
}

@ public
fn require(Scope scope, string selector) → Element {
    return Element { selector = selector }
}

@ public
fn text_set(Element element, string value) → void {
    print element
    print value
}

@ public
fn on_submit(Element element, SubmitOptions options, SubmitHandler handler) → Subscription {
    print element
    print options
    print handler
    return Subscription { id = 0 }
}

@ public
fn on_frame(FrameHandler handler) → Subscription {
    print handler
    return Subscription { id = 0 }
}
"#,
    )
    .expect("dom.fab");
    fs::write(
        lib.join("bindings/ts.toml"),
        r#"[shim]
path = "runtime/dom.ts"

[shims.web]
path = "runtime/web.ts"

[functions."tela:web.noop"]
symbol = "webNoop"

[functions."tela:dom.scope"]
symbol = "webDomScope"

[functions."tela:dom.require"]
symbol = "webDomRequire"

[functions."tela:dom.text_set"]
symbol = "webDomTextSet"

[functions."tela:dom.on_submit"]
symbol = "webDomOnSubmit"

[functions."tela:dom.on_frame"]
symbol = "webDomOnFrame"
"#,
    )
    .expect("ts.toml");
    fs::write(
        lib.join("runtime/dom.ts"),
        r"export type WebDomScope = { readonly root: ParentNode; readonly selector: string };
export type WebDomElement = { readonly selector: string };
export type WebDomEvent = { readonly kind: string };
export type WebDomFrameState = { readonly frame: number };
export type WebDomResizeState = { readonly width: number };
export type WebDomKeyboardState = { readonly key: string };
export type WebDomPointerState = { readonly x: number };
export type WebDomFocusState = { readonly focused: boolean };
export type WebDomPointerLockState = { readonly locked: boolean };
export type WebDomSubscription = { unsubscribe(): void };
export type WebDomSubmitOptions = { readonly prevent_default: boolean };
export type WebDomFetchRequest = { readonly url: string };
export type WebDomFetchResponse = { readonly status: number };
export type WebDomEventHandler = (event: WebDomEvent) => void;
export type WebDomInputHandler = (element: WebDomElement, value: string) => void;
export type WebDomSubmitHandler = (form: WebDomElement) => void;
export type WebDomFrameHandler = (state: WebDomFrameState) => void;
export type WebDomResizeHandler = (state: WebDomResizeState) => void;
export type WebDomKeyboardHandler = (state: WebDomKeyboardState) => void;
export type WebDomPointerHandler = (state: WebDomPointerState) => void;
export type WebDomFocusHandler = (state: WebDomFocusState) => void;
export type WebDomPointerLockHandler = (state: WebDomPointerLockState) => void;

export function webDomScope(selector: string): WebDomScope {
    return { root: globalThis.document, selector };
}

export function webDomRequire(scope: WebDomScope, selector: string): WebDomElement {
    return { selector };
}

export function webDomTextSet(element: WebDomElement, value: string): void {
    void element;
    void value;
}

export function webDomOnSubmit(
    form: WebDomElement,
    options: WebDomSubmitOptions,
    handler: WebDomSubmitHandler,
): WebDomSubscription {
    void form;
    void options;
    void handler;
    return { unsubscribe() {} };
}

export function webDomOnFrame(handler: WebDomFrameHandler): WebDomSubscription {
    void handler;
    return { unsubscribe() {} };
}
",
    )
    .expect("dom shim");
}

/// Import only `tela:web` and define a local `Scope` for shadowing tests.
fn write_tela_consumer_app_with_only_web_import(app: &Path, lib: &Path) {
    write_tela_consumer_app(
        app,
        lib,
        r##"
import from "tela:web" web

class Scope {
    mut string selector = ""
}

@ WebController { selector = "#shell" }
fn shell(Scope scope) → void {}
"##,
    );
}

#[test]
fn g10_web1_tela_package_exports_controller_contract() {
    let root = test_temp_dir("g10-web1-tela-contract");
    let lib = root.join("tela");
    write_synthetic_tela_lib(&lib);

    let package = analyze_with_faber_seams(&Config::default(), &lib).expect("analyze tela package");
    let unit = package
        .nodes
        .values()
        .find(|unit| unit.path.ends_with("web.fab"))
        .expect("web.fab unit");
    let export = unit
        .file_interface
        .exports
        .get("WebController")
        .expect("WebController export");
    let FileExportKind::Struct(strukt) = &export.kind else {
        panic!("WebController must be a struct export");
    };
    let contract = strukt
        .annotation_contract
        .as_ref()
        .expect("WebController must carry annotation_contract");
    // Annotation contracts store the Latin identity surface (`functio` /
    // `textus`), even when the package is authored in en.
    assert_eq!(contract.target, "functio");
    assert_eq!(contract.fields.len(), 1);
    assert_eq!(contract.fields[0].name, "selector");
    assert_eq!(contract.fields[0].ty, "textus");
    let identity = contract
        .qualified_identity
        .as_ref()
        .expect("WebController identity must be qualified");
    assert_eq!(identity.provider, "package");
    assert_eq!(identity.package.as_deref(), Some("tela"));
    assert_eq!(identity.export_name, "WebController");
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web2_product_static_assets_are_deterministic_and_manifested() {
    let dir = test_temp_dir("g10-web2-static-assets");
    write_browser_product_manifest(&dir, r"");
    write_static_asset_roots(&dir);
    fs::write(dir.join("pages/index.html"), "<main>Hello</main>\n").expect("page");
    fs::write(dir.join("styles/site.css"), "main { color: red; }\n").expect("style");
    fs::write(dir.join("public/logo.svg"), "<svg/>\n").expect("public");

    let manifest = read_manifest(&dir.join("faber.toml")).expect("manifest");
    let first = build_browser_product_static_assets(&dir, manifest.product.as_ref().unwrap())
        .expect("first static build");
    let first_manifest = fs::read_to_string(&first.manifest_path).expect("first manifest");
    let first_assets = first
        .assets
        .iter()
        .map(|asset| {
            (
                asset.kind,
                asset
                    .output
                    .strip_prefix(&first.out_dir)
                    .unwrap()
                    .to_path_buf(),
                asset.sha256.clone(),
            )
        })
        .collect::<Vec<_>>();

    let second = build_browser_product_static_assets(&dir, manifest.product.as_ref().unwrap())
        .expect("second static build");
    let second_manifest = fs::read_to_string(&second.manifest_path).expect("second manifest");
    let second_assets = second
        .assets
        .iter()
        .map(|asset| {
            (
                asset.kind,
                asset
                    .output
                    .strip_prefix(&second.out_dir)
                    .unwrap()
                    .to_path_buf(),
                asset.sha256.clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(first_manifest, second_manifest);
    assert_eq!(first_assets, second_assets);
    assert!(first.out_dir.join("pages/index.html").is_file());
    assert!(first.out_dir.join("styles/site.css").is_file());
    assert!(first.out_dir.join("public/logo.svg").is_file());
    assert!(first_manifest.contains("pages/index.html"));
    assert!(first_manifest.contains("styles/site.css"));
    assert!(first_manifest.contains("public/logo.svg"));
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web2_product_static_assets_fail_closed_on_missing_root_collision_and_stale_output() {
    let missing = test_temp_dir("g10-web2-missing-root");
    write_browser_product_manifest(&missing, r"");
    fs::create_dir_all(missing.join("pages")).expect("pages");
    fs::create_dir_all(missing.join("styles")).expect("styles");
    let manifest = read_manifest(&missing.join("faber.toml")).expect("manifest");
    let err = build_browser_product_static_assets(&missing, manifest.product.as_ref().unwrap())
        .expect_err("missing public root fails closed");
    assert!(diagnostic_has_issue(&err, "product_asset_root_missing"));

    let collision = test_temp_dir("g10-web2-collision");
    write_browser_product_manifest(
        &collision,
        r#"
templates = "pages"
styles = "pages"
public = "public"
"#,
    );
    write_static_asset_roots(&collision);
    fs::write(
        collision.join("pages/shared.txt"),
        "same source, same output\n",
    )
    .expect("shared");
    let manifest = read_manifest(&collision.join("faber.toml")).expect("manifest");
    let err = build_browser_product_static_assets(&collision, manifest.product.as_ref().unwrap())
        .expect_err("duplicate output fails closed");
    assert!(diagnostic_has_issue(&err, "product_asset_collision"));

    let stale = test_temp_dir("g10-web2-stale-output");
    write_browser_product_manifest(&stale, r"");
    write_static_asset_roots(&stale);
    fs::write(stale.join("pages/index.html"), "ok\n").expect("page");
    fs::create_dir_all(stale.join("dist/pages")).expect("dist pages");
    fs::write(stale.join("dist/pages/old.html"), "stale\n").expect("stale");
    let manifest = read_manifest(&stale.join("faber.toml")).expect("manifest");
    let err = build_browser_product_static_assets(&stale, manifest.product.as_ref().unwrap())
        .expect_err("stale output fails closed");
    assert!(diagnostic_has_issue(&err, "product_stale_output"));

    // Manifest path collides with a planned asset output → fail closed.
    let manifest_collision = test_temp_dir("g10-web2-manifest-collision");
    fs::write(
        manifest_collision.join("faber.toml"),
        r#"[package]
name = "web2-manifest-collision"

[paths]
entry = "main.fab"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "pages"
styles = "styles"
public = "public"
assets_manifest = "pages/index.html"
"#,
    )
    .expect("manifest");
    write_static_asset_roots(&manifest_collision);
    fs::write(
        manifest_collision.join("pages/index.html"),
        "manifest will overwrite me\n",
    )
    .expect("colliding asset");
    let manifest = read_manifest(&manifest_collision.join("faber.toml")).expect("manifest");
    let err = build_browser_product_static_assets(
        &manifest_collision,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("manifest/asset collision fails closed");
    assert!(diagnostic_has_issue(&err, "product_output_collision"));

    // Controllers-json path collides with a planned asset output → fail closed.
    // Regression for auditor-1 block_ship on b0c0e3d: the manifest collision
    // guard existed, but controllers_json was unchecked.
    let controllers_collision = test_temp_dir("g10-web2-controllers-collision");
    fs::write(
        controllers_collision.join("faber.toml"),
        r#"[package]
name = "web2-controllers-collision"

[paths]
entry = "main.fab"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "pages"
styles = "styles"
public = "public"
controllers_json = "pages/index.html"
"#,
    )
    .expect("manifest");
    write_static_asset_roots(&controllers_collision);
    fs::write(
        controllers_collision.join("pages/index.html"),
        "controllers will overwrite me\n",
    )
    .expect("colliding asset");
    let manifest = read_manifest(&controllers_collision.join("faber.toml")).expect("manifest");
    let err = build_browser_product_static_assets(
        &controllers_collision,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("controllers_json/asset collision fails closed");
    assert!(diagnostic_has_issue(&err, "product_output_collision"));

    // Controllers-json path equals assets-manifest path → fail closed.
    let self_collision = test_temp_dir("g10-web2-output-self-collision");
    fs::write(
        self_collision.join("faber.toml"),
        r#"[package]
name = "web2-output-self-collision"

[paths]
entry = "main.fab"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "pages"
styles = "styles"
public = "public"
assets_manifest = "shared.json"
controllers_json = "shared.json"
"#,
    )
    .expect("manifest");
    write_static_asset_roots(&self_collision);
    let manifest = read_manifest(&self_collision.join("faber.toml")).expect("manifest");
    let err =
        build_browser_product_static_assets(&self_collision, manifest.product.as_ref().unwrap())
            .expect_err("generated output self-collision fails closed");
    assert!(diagnostic_has_issue(&err, "product_output_collision"));

    // Static asset inside generated directory (faber-ts) → fail closed.
    // Regression for auditor-1 block_ship 272ca15: equality-only collision
    // check did not reject static assets whose output lands inside a
    // generated directory. The generated `faber-ts/` owns its subtree, so any
    // planned static asset at `dist/faber-ts/**` must collide.
    let generated_dir_collision = test_temp_dir("g10-web2-generated-dir-collision");
    fs::write(
        generated_dir_collision.join("faber.toml"),
        r#"[package]
name = "web2-generated-dir-collision"

[paths]
entry = "main.fab"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "faber-ts"
styles = "styles"
public = "public"
"#,
    )
    .expect("manifest");
    fs::create_dir_all(generated_dir_collision.join("faber-ts")).expect("faber-ts source");
    fs::create_dir_all(generated_dir_collision.join("styles")).expect("styles");
    fs::create_dir_all(generated_dir_collision.join("public")).expect("public");
    fs::write(
        generated_dir_collision.join("faber-ts/main.ts"),
        "STATIC MAIN TS SENTINEL\n",
    )
    .expect("static inside generated dir");
    let manifest = read_manifest(&generated_dir_collision.join("faber.toml")).expect("manifest");
    let err = build_browser_product_static_assets(
        &generated_dir_collision,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("static asset inside generated directory fails closed");
    assert!(diagnostic_has_issue(&err, "product_output_collision"));

    // Static asset inside generated directory (faber-esm) → fail closed.
    let esm_collision = test_temp_dir("g10-web2-esm-dir-collision");
    fs::write(
        esm_collision.join("faber.toml"),
        r#"[package]
name = "web2-esm-dir-collision"

[paths]
entry = "main.fab"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "pages"
styles = "styles"
public = "faber-esm"
"#,
    )
    .expect("manifest");
    fs::create_dir_all(esm_collision.join("pages")).expect("pages");
    fs::create_dir_all(esm_collision.join("styles")).expect("styles");
    fs::create_dir_all(esm_collision.join("faber-esm")).expect("faber-esm source");
    fs::write(esm_collision.join("faber-esm/polyfill.js"), "export {}\n")
        .expect("static inside generated dir");
    let manifest = read_manifest(&esm_collision.join("faber.toml")).expect("manifest");
    let err =
        build_browser_product_static_assets(&esm_collision, manifest.product.as_ref().unwrap())
            .expect_err("static asset inside generated esm directory fails closed");
    assert!(diagnostic_has_issue(&err, "product_output_collision"));
}

fn write_browser_product_manifest(root: &Path, product_overrides: &str) {
    fs::write(
        root.join("faber.toml"),
        format!(
            r#"[package]
name = "web2-static"

[paths]
entry = "main.fab"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
assets_manifest = "faber-assets.json"
{product_overrides}
"#
        ),
    )
    .expect("manifest");
}

fn write_static_asset_roots(root: &Path) {
    fs::create_dir_all(root.join("pages")).expect("pages");
    fs::create_dir_all(root.join("styles")).expect("styles");
    fs::create_dir_all(root.join("public")).expect("public");
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_builds_controllers_json_and_browser_esm() {
    if Command::new("tsc").arg("--version").output().is_err() {
        eprintln!("tsc not found on PATH; skipping WEB3 browser ESM test");
        return;
    }
    let root = test_temp_dir("g10-web3-browser-esm");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let app = root.join("app");
    write_tela_consumer_app(
        &app,
        &lib,
        r#"
import from "tela:web" web
import from "tela:dom" d

@ WebController { selector = "[data-faber=shell]" }
#[cfg(feature = "hir-ts")]
fn shell(d.Scope scope) → void {
  print d.require(scope, "button")
}
"#,
    );
    write_static_asset_roots(&app);
    fs::write(
        app.join("pages/index.html"),
        "<main data-faber=shell></main>\n",
    )
    .expect("page");

    let manifest = read_manifest(&app.join("faber.toml")).expect("manifest");
    let build = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        manifest.product.as_ref().unwrap(),
    )
    .expect("browser product build");

    let controllers = fs::read_to_string(&build.controllers_json).expect("controllers.json");
    assert!(controllers.contains("\"selector\": \"[data-faber=shell]\""));
    assert!(controllers.contains("\"module\": \"./main.js\""));
    assert!(build.esm_entry.is_file());
    let esm = fs::read_to_string(&build.esm_entry).expect("esm entry");
    assert!(esm.contains("./main.js"));
    assert!(esm.contains("export function mountControllers"));
    assert!(esm.contains("failures.push"));
    assert!(esm.contains("disposeCleanup"));
    let ambient = fs::read_to_string(build.out_dir.join("faber-ts/faber-browser-ambient.d.ts"))
        .expect("ambient browser declarations");
    assert!(ambient.contains("export class FrameState"));
    assert!(ambient.contains("on_frame(handler: FrameHandler): Subscription"));
    assert_eq!(build.controllers.len(), 1);

    let second = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        manifest.product.as_ref().unwrap(),
    )
    .expect("browser product rebuild");
    assert_eq!(
        fs::read_to_string(&second.controllers_json).unwrap(),
        controllers
    );
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_rejects_duplicate_mount_and_invalid_selector() {
    let root = test_temp_dir("g10-web3-controller-rejects");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let dup = root.join("dup");
    write_tela_consumer_app(
        &dup,
        &lib,
        r##"
import from "tela:web" web
import from "tela:dom" d

@ WebController { selector = "#shell" }
fn one(d.Scope scope) → void {}

@ WebController { selector = "#shell" }
#[cfg(feature = "hir-ts")]
fn two(d.Scope scope) → void {}
"##,
    );
    write_static_asset_roots(&dup);
    let manifest = read_manifest(&dup.join("faber.toml")).expect("manifest");
    let err = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &dup,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("duplicate mount fails closed");
    assert!(diagnostic_has_issue(&err, "product_duplicate_mount"));

    let invalid = root.join("invalid");
    write_tela_consumer_app(
        &invalid,
        &lib,
        r#"
import from "tela:web" web
import from "tela:dom" d

@ WebController { selector = "main shell" }
#[cfg(feature = "hir-ts")]
fn shell(d.Scope scope) → void {}
"#,
    );
    write_static_asset_roots(&invalid);
    let manifest = read_manifest(&invalid.join("faber.toml")).expect("manifest");
    let err = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &invalid,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("invalid static selector fails closed");
    assert!(diagnostic_has_issue(
        &err,
        "product_invalid_static_selector"
    ));
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_rejects_local_web_controller_shadowing() {
    // Package defines its own @annotatio WebController locally — no web:web import.
    // Controller discovery must reject the unqualified origin.
    let root = test_temp_dir("g10-web3-local-controller");
    fs::create_dir_all(root.join("src")).expect("src");
    write_static_asset_roots(&root);
    fs::write(
        root.join("faber.toml"),
        r#"[package]
name = "local-controller"
version = "0.1.0"

[paths]
entry = "main.fab"

[reader]
locale = "la"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
"#,
    )
    .expect("manifest");
    fs::write(
        root.join("src/main.fab"),
        r##"
@ annotatio
genus WebController {
    varia textus selector
}

genus Scope {
    varia textus selector = ""
}

@ WebController { selector = "#shell" }
functio shell(Scope scope) → vacuum {}
"##,
    )
    .expect("entry");
    let manifest = read_manifest(&root.join("faber.toml")).expect("manifest");
    let err = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &root,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("local WebController must fail closed");
    assert!(diagnostic_has_issue(
        &err,
        "product_controller_unqualified_origin"
    ));
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_rejects_local_scope_shadowing() {
    // Package imports tela:web legitimately for WebController, but defines its
    // own Scope genus locally instead of importing tela:dom Scope.
    let root = test_temp_dir("g10-web3-local-scope");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let app = root.join("app");
    write_tela_consumer_app_with_only_web_import(&app, &lib);
    write_static_asset_roots(&app);
    let manifest = read_manifest(&app.join("faber.toml")).expect("manifest");
    let err = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("local Scope must fail closed");
    assert!(diagnostic_has_issue(
        &err,
        "product_invalid_controller_signature"
    ));
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_rejects_missing_controllers() {
    // Package imports tela:web legitimately but declares no `@ WebController`
    // function at all — controller discovery must fail closed with
    // `product_controller_missing` rather than emitting an empty browser
    // product.
    let root = test_temp_dir("g10-web3-missing-controllers");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let app = root.join("app");
    write_tela_consumer_app(
        &app,
        &lib,
        r#"
import from "tela:web" web
import from "tela:dom" d

#[cfg(feature = "hir-ts")]
fn helper(d.Scope scope) → void {
  print d.require(scope, "button")
}
"#,
    );
    write_static_asset_roots(&app);
    let manifest = read_manifest(&app.join("faber.toml")).expect("manifest");
    let err = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("browser product without WebController functions must fail closed");
    assert!(diagnostic_has_issue(&err, "product_controller_missing"));
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_builds_tela_origin_controllers_json_and_browser_esm() {
    // web-mig U2: a consumer that imports `tela:web` / `tela:dom` (instead of
    // `web:web` / `web:dom`) must be accepted as a controller origin and
    // build a browser product that produces `controllers.json`.
    if Command::new("tsc").arg("--version").output().is_err() {
        eprintln!("tsc not found on PATH; skipping tela-origin browser ESM test");
        return;
    }
    let root = test_temp_dir("g10-web3-tela-origin-esm");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let app = root.join("app");
    write_tela_consumer_app(
        &app,
        &lib,
        r#"
import from "tela:web" web
import from "tela:dom" d

@ WebController { selector = "[data-faber=shell]" }
#[cfg(feature = "hir-ts")]
fn shell(d.Scope scope) → void {}
"#,
    );
    write_static_asset_roots(&app);
    fs::write(
        app.join("pages/index.html"),
        "<main data-faber=shell></main>\n",
    )
    .expect("page");

    let manifest = read_manifest(&app.join("faber.toml")).expect("manifest");
    let build = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        manifest.product.as_ref().unwrap(),
    )
    .expect("tela-origin browser product build");

    let controllers = fs::read_to_string(&build.controllers_json).expect("controllers.json");
    assert!(controllers.contains("\"selector\": \"[data-faber=shell]\""));
    assert!(controllers.contains("\"module\": \"./main.js\""));
    assert!(build.esm_entry.is_file());
    let tela_web_js = build.out_dir.join("faber-esm/tela-web.js");
    assert!(
        tela_web_js.is_file(),
        "Tela web module missing: {}",
        tela_web_js.display()
    );
    assert!(
        fs::read_to_string(tela_web_js)
            .expect("tela web module")
            .contains("webNoop")
    );
    assert_eq!(build.controllers.len(), 1);
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_rejects_local_web_controller_shadowing_with_tela_import() {
    // web-mig U2: a tela-importing package that defines its own local
    // `@ annotation WebController` (instead of importing the `tela:web`
    // contract) must still fail closed — the tela-origin acceptance must not
    // weaken the no-local-shadowing rejection.
    let root = test_temp_dir("g10-web3-tela-local-controller");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let app = root.join("app");
    write_tela_consumer_app(
        &app,
        &lib,
        r##"
import from "tela:dom" d

@ annotation { target = fn }
class WebController {
    mut string selector
}

@ WebController { selector = "#shell" }
#[cfg(feature = "hir-ts")]
fn shell(d.Scope scope) → void {}
"##,
    );
    write_static_asset_roots(&app);
    let manifest = read_manifest(&app.join("faber.toml")).expect("manifest");
    let err = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("local WebController must fail closed in a tela-importing package");
    assert!(diagnostic_has_issue(
        &err,
        "product_controller_unqualified_origin"
    ));
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_rejects_missing_controllers_with_tela_import() {
    // web-mig U2: a package imports tela:web + tela:dom legitimately but
    // declares no `@ WebController` function at all — controller discovery
    // must still fail closed with `product_controller_missing`.
    let root = test_temp_dir("g10-web3-tela-missing-controllers");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let app = root.join("app");
    write_tela_consumer_app(
        &app,
        &lib,
        r#"
import from "tela:web" web
import from "tela:dom" d

#[cfg(feature = "hir-ts")]
fn helper(d.Scope scope) → void {}
"#,
    );
    write_static_asset_roots(&app);
    let manifest = read_manifest(&app.join("faber.toml")).expect("manifest");
    let err = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        manifest.product.as_ref().unwrap(),
    )
    .expect_err("browser product without WebController functions must fail closed");
    assert!(diagnostic_has_issue(&err, "product_controller_missing"));
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_rewrites_unresolved_def_controller_typescript() {
    // Imported nominal types (e.g. `dom.FrameState`, `dom.SubmitOptions`) have
    // no DefId in the consuming unit's name table, so Radix's TS printer emits
    // the `unresolved_def` sentinel in type positions and `new unresolved_def()`
    // for struct construction. `adapt_controller_typescript` must rewrite those
    // to `any` / `{}` so the emitted controller module passes `tsc`.
    if Command::new("tsc").arg("--version").output().is_err() {
        eprintln!("tsc not found on PATH; skipping WEB3 unresolved_def test");
        return;
    }
    let root = test_temp_dir("g10-web3-unresolved-def");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let app = root.join("app");
    write_tela_consumer_app(
        &app,
        &lib,
        r##"
import from "tela:web" web
import from "tela:dom" d

@ WebController { selector = "#frame-demo" }
fn frame_controller(d.Scope scope) → void {
  let d.Element status ← d.require(scope, ".frame-status")
  print d.on_frame((d.FrameState frame) → void ∴ d.text_set(status, "frame-seen"))
}

@ WebController { selector = "#submit-demo" }
#[cfg(feature = "hir-ts")]
fn submit_controller(d.Scope scope) → void {
  let d.Element form ← d.require(scope, ".submit-form")
  print d.on_submit(form, d.SubmitOptions { prevent_default = true }, (d.Element el) → void ∴ d.text_set(el, "submitted"))
}
"##,
    );
    write_static_asset_roots(&app);

    let manifest = read_manifest(&app.join("faber.toml")).expect("manifest");
    let build = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        manifest.product.as_ref().unwrap(),
    )
    .expect("browser product build with unresolved_def rewrites (tsc must pass)");

    let main_ts = fs::read_to_string(build.out_dir.join("faber-ts/main.ts"))
        .expect("emitted controller module");
    assert!(
        !main_ts.contains("unresolved_def"),
        "adapted controller TypeScript must not contain unresolved_def:\n{main_ts}"
    );
    assert!(
        main_ts.contains("export function frame_controller("),
        "controller export rewrite must survive adaptation:\n{main_ts}"
    );
    assert_eq!(build.controllers.len(), 2);
}

// ---------------------------------------------------------------------------
// FBR-P2-005: atomic browser product publication (Stage 3)
// ---------------------------------------------------------------------------

#[cfg(feature = "hir-ts")]
fn assert_no_product_staging_temps(app: &Path, out_name: &str) {
    let prefix_tmp = format!(".{out_name}.faber.tmp-");
    let prefix_old = format!(".{out_name}.faber.old-");
    for entry in fs::read_dir(app).expect("read app dir").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !(name.starts_with(&prefix_tmp) || name.starts_with(&prefix_old)),
            "leftover product staging directory `{name}`"
        );
    }
}

#[cfg(feature = "hir-ts")]
fn sample_web_app() -> (TestDir, PathBuf) {
    let root = test_temp_dir("g10-web3-transaction");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let app = root.join("app");
    write_tela_consumer_app(
        &app,
        &lib,
        r#"
import from "tela:web" web
import from "tela:dom" d

@ WebController { selector = "[data-faber=shell]" }
fn shell(d.Scope scope) → void {
  print d.require(scope, "button")
}
"#,
    );
    write_static_asset_roots(&app);
    fs::write(
        app.join("pages/index.html"),
        "<main data-faber=shell></main>\n",
    )
    .expect("page");
    (root, app)
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_failed_rebuild_keeps_previous_product_usable() {
    if Command::new("tsc").arg("--version").output().is_err() {
        eprintln!("tsc not found on PATH; skipping WEB3 failure-injection test");
        return;
    }
    let (_root, app) = sample_web_app();
    let manifest = read_manifest(&app.join("faber.toml")).expect("manifest");
    let product = manifest.product.as_ref().expect("product");
    let build = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        product,
    )
    .expect("first product build");
    let first_controllers = fs::read_to_string(&build.controllers_json).expect("controllers");
    assert!(build.esm_entry.is_file());

    // Inject a failure after each major build stage; the previous product must
    // remain complete and usable, and no partial new product may appear.
    for stage in 1..=5 {
        inject_product_failure_at(stage);
        let err = build_browser_product(
            &Config::default().with_target(Target::HirTypeScript),
            &app,
            product,
        )
        .expect_err("injected failure must abort the product build");
        assert!(
            diagnostic_has_issue(&err, "product_injected_failure"),
            "stage {stage}: unexpected error: {err:?}"
        );
        let controllers_after = fs::read_to_string(&build.controllers_json).expect("controllers");
        assert_eq!(
            controllers_after, first_controllers,
            "previous product changed at stage {stage}"
        );
        assert!(
            build.esm_entry.is_file(),
            "previous esm entry lost at stage {stage}"
        );
        let product_json =
            fs::read_to_string(build.out_dir.join("product.json")).expect("product.json");
        assert!(
            product_json.contains("\"version\": 1"),
            "previous product manifest lost at stage {stage}"
        );
        assert_no_product_staging_temps(&app, "dist");
    }
    inject_product_failure_at(0);

    // A clean rebuild after the injected failures still succeeds and swaps.
    let rebuilt = build_browser_product(
        &Config::default().with_target(Target::HirTypeScript),
        &app,
        product,
    )
    .expect("rebuild after reset");
    assert_eq!(
        fs::read_to_string(&rebuilt.controllers_json).expect("controllers"),
        first_controllers
    );
    assert_no_product_staging_temps(&app, "dist");
}

#[cfg(feature = "hir-ts")]
#[test]
fn g10_web3_first_build_failure_leaves_no_partial_product() {
    if Command::new("tsc").arg("--version").output().is_err() {
        eprintln!("tsc not found on PATH; skipping WEB3 failure-injection test");
        return;
    }
    let (_root, app) = sample_web_app();
    let manifest = read_manifest(&app.join("faber.toml")).expect("manifest");
    let product = manifest.product.as_ref().expect("product");

    for stage in 1..=5 {
        inject_product_failure_at(stage);
        let err = build_browser_product(
            &Config::default().with_target(Target::HirTypeScript),
            &app,
            product,
        )
        .expect_err("injected failure must abort the product build");
        assert!(diagnostic_has_issue(&err, "product_injected_failure"));
        // No final output directory may appear for a failed first build.
        assert!(
            !app.join("dist").exists(),
            "partial product directory leaked at stage {stage}"
        );
        assert_no_product_staging_temps(&app, "dist");
    }
    inject_product_failure_at(0);
}

#[test]
fn g10_web1_imported_web_controller_survives_package_graph_extract() {
    let root = test_temp_dir("g10-web-imported-controller");
    let lib = root.join("tela-lib");
    write_synthetic_tela_lib(&lib);
    let app = root.join("app");
    write_tela_consumer_app(
        &app,
        &lib,
        r#"
import from "tela:web" web
import from "tela:dom" d

@ WebController { selector = "[data-faber=shell]" }
fn shell(d.Scope scope) → void {
  print d.require(scope, "button")
}

main {
  shell(d.scope("[data-faber=shell]"))
}
"#,
    );

    let package =
        analyze_with_faber_seams(&Config::default(), &app).expect("analyze web app package graph");
    let entry = package.entry_node().expect("entry unit");
    let application = entry
        .analysis
        .annotation_contracts
        .applications()
        .next()
        .expect("imported WebController contract application");
    assert_eq!(
        entry.analysis.interner.resolve(application.family),
        "WebController"
    );
    let contract = entry
        .analysis
        .annotation_contracts
        .registry
        .get(application.contract_id)
        .expect("imported WebController contract");
    assert_eq!(
        entry.analysis.interner.resolve(contract.name),
        "WebController"
    );
    assert_eq!(contract.fields.len(), 1);
    assert_eq!(
        entry.analysis.interner.resolve(contract.fields[0].name),
        "selector"
    );
}

// ---------------------------------------------------------------------------
// R0 red artifact contract: FMIR text/binary artifact version
//
// `PACKAGE_MIR_ARTIFACT_VERSION` is the exact-version gate for package MIR
// images. The version is a clean-break pin: version-8 images round-trip,
// and older (2) or future (999) images fail closed through the existing
// exact-version gate with the expected-version diagnostic.
// ---------------------------------------------------------------------------

fn rewrite_text_image_version(text: &str, version: u32) -> String {
    for current in [
        "version = 2",
        "version = 3",
        "version = 4",
        "version = 5",
        "version = 6",
        "version = 7",
        "version = 8",
    ] {
        if text.contains(current) {
            return text.replacen(current, &format!("version = {version}"), 1);
        }
    }
    panic!("image text has no recognizable version line:\n{text}")
}

fn rewrite_binary_image_version(bytes: &mut [u8], version: u8) {
    assert!(
        matches!(
            bytes.first(),
            Some(2) | Some(3) | Some(4) | Some(5) | Some(6) | Some(7) | Some(8)
        ),
        "binary image must start with a recognizable artifact version varint"
    );
    bytes[0] = version;
}

#[test]
fn package_fmir_text_image_is_artifact_version_8() {
    let dir = test_temp_dir("package-fmir-text-v8");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"v7\" }").expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");

    assert!(
        image_text.contains("version = 8"),
        "fmir-text image must declare artifact version 8:\n{image_text}"
    );
}

#[test]
fn package_fmir_image_is_artifact_version_8() {
    let dir = test_temp_dir("package-fmir-v8");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"v7\" }").expect("write entry");

    let image = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    )
    .expect("build fmir image");
    let bytes = fs::read(&image.image_path).expect("read image");
    let summary = fmir_image_test_summary(&bytes, &image.image_path).expect("summarize fmir image");

    assert_eq!(
        summary.version, 8,
        "fmir image must declare artifact version 8"
    );
}

#[test]
fn package_fmir_text_image_rejects_version_2_images() {
    let dir = test_temp_dir("package-fmir-text-v2-reject");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"must not run\" }").expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");
    fs::write(
        &image.image_path,
        rewrite_text_image_version(&image_text, 2),
    )
    .expect("write version-2 image");
    fs::remove_file(&entry).expect("remove source after rewriting image");

    let mut host = BufferHost::default();
    let diagnostics = run_fmir_text_image_for_faber(&image, &mut host)
        .expect_err("version-2 fmir-text images must fail closed under the version-8 schema");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_text_image_version_unsupported")
            && diagnostic_has_arg(diag, "actual", "2")
            && diagnostic_has_arg(diag, "expected", "8")
    }));
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

#[test]
fn package_fmir_text_image_rejects_future_versions() {
    let dir = test_temp_dir("package-fmir-text-future-reject");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"must not run\" }").expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");
    fs::write(
        &image.image_path,
        rewrite_text_image_version(&image_text, 999),
    )
    .expect("write future-version image");
    fs::remove_file(&entry).expect("remove source after rewriting image");

    let mut host = BufferHost::default();
    let diagnostics = run_fmir_text_image_for_faber(&image, &mut host)
        .expect_err("future fmir-text versions must fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_text_image_version_unsupported")
            && diagnostic_has_arg(diag, "actual", "999")
            && diagnostic_has_arg(diag, "expected", "8")
    }));
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

#[test]
fn package_fmir_image_rejects_version_2_images() {
    let dir = test_temp_dir("package-fmir-v2-reject");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"must not run\" }").expect("write entry");

    let image = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    )
    .expect("build fmir image");
    let mut image_bytes = fs::read(&image.image_path).expect("read image");
    rewrite_binary_image_version(&mut image_bytes, 2);
    fs::write(&image.image_path, image_bytes).expect("write version-2 image");
    fs::remove_file(&entry).expect("remove source after rewriting image");

    let mut host = BufferHost::default();
    let diagnostics = run_fmir_image_for_faber(&image, &mut host)
        .expect_err("version-2 fmir images must fail closed under the version-8 schema");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_image_version_unsupported")
            && diagnostic_has_arg(diag, "actual", "2")
            && diagnostic_has_arg(diag, "expected", "8")
    }));
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

#[test]
fn package_fmir_image_rejects_future_versions() {
    let dir = test_temp_dir("package-fmir-future-reject");
    let entry = dir.join("main.fab");
    fs::write(&entry, "main { print \"must not run\" }").expect("write entry");

    let image = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    )
    .expect("build fmir image");
    let mut image_bytes = fs::read(&image.image_path).expect("read image");
    rewrite_binary_image_version(&mut image_bytes, 99);
    fs::write(&image.image_path, image_bytes).expect("write future-version image");
    fs::remove_file(&entry).expect("remove source after rewriting image");

    let mut host = BufferHost::default();
    let diagnostics = run_fmir_image_for_faber(&image, &mut host)
        .expect_err("future fmir versions must fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_image_version_unsupported")
            && diagnostic_has_arg(diag, "actual", "99")
            && diagnostic_has_arg(diag, "expected", "8")
    }));
    assert_eq!(host.stdout_lines, [] as [String; 0]);
}

#[test]
fn package_fmir_text_image_round_trips_upper_half_u64_constants() {
    // R0 red carrier proof at the artifact boundary: an unsigned 64-bit
    // constant above i64::MAX must survive image build, source removal, and
    // image execution with its exact value. Fails until R1 (literal carrier)
    // and R2 (`MirConstant::UInt` + version 3) land.
    let dir = test_temp_dir("package-fmir-text-u64-constants");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r"
main {
  let u64 max ← 18446744073709551615
  let u64 sha ← 9223372036854775808
  print max
  print sha
}
",
    )
    .expect("write entry");

    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image with upper-half u64 constants");
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::default();
    run_fmir_text_image_for_faber(&image, &mut host).expect("run fmir-text image without source");

    assert_eq!(
        host.stdout_lines,
        vec![
            "18446744073709551615".to_owned(),
            "9223372036854775808".to_owned()
        ]
    );
}

#[test]
fn package_fmir_image_round_trips_upper_half_u64_constants() {
    let dir = test_temp_dir("package-fmir-u64-constants");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r"
main {
  let u64 max ← 18446744073709551615
  let u64 sha ← 9223372036854775808
  print max
  print sha
}
",
    )
    .expect("write entry");

    let image = build_fmir_image_for_faber(
        &Config::default().with_target(Target::MirFmirBinary),
        &entry,
        &[],
    )
    .expect("build fmir image with upper-half u64 constants");
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::default();
    run_fmir_image_for_faber(&image, &mut host).expect("run fmir image without source");

    assert_eq!(
        host.stdout_lines,
        vec![
            "18446744073709551615".to_owned(),
            "9223372036854775808".to_owned()
        ]
    );
}

#[test]
fn library_interface_exports_backward_companion() {
    // SEM004: `@ radix backward` companions of a `@ public` function share
    // its export tier (D5.1), so they must appear in the library file
    // interface so consumers can call them across `importa` boundaries
    // without a mirrored annotation. Covers both the parse-level export-name
    // collection (program_export_names via package analysis) and the
    // analyzed-program extractor (companion signature snapshot).
    let dir = test_temp_dir("gradus-companion-export");
    fs::create_dir_all(dir.join("src")).expect("create source");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "testgrad"
version = "0.1.0"
edition = "2026"

[library]
provider = "testgrad"

[paths]
source = "src"

[build]
kind = "lib"
target = "fmir"
targets = ["fmir"]
"#,
    )
    .expect("write manifest");
    fs::write(
        dir.join("src/gradient.fab"),
        r#"fn nil() → void {
    pass
}

@ public
@ radix lane "air"
@ radix backward "loss_backward"
fn simple_loss(
    tensor<f32, [2,2]> x,
    tensor<f32, [2,2]> w
) → f32 {
    let tensor<f32, [2,2]> product ← x ⊙ w
    return product.mean()
}
"#,
    )
    .expect("write module");

    let config = Config {
        target: radix::module::codegen::Target::MirFmirBinary,
        ..Config::default()
    };
    let package = analyze_with_faber_seams(&config, &dir).expect("analyze library package");
    assert!(
        !package
            .diagnostics
            .iter()
            .any(radix::module::Diagnostic::is_error),
        "expected no analysis errors, got {:?}",
        package
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );

    let gradient = package
        .nodes
        .values()
        .find(|unit| unit.module_segments == vec!["gradient".to_owned()])
        .expect("gradient unit");

    assert!(
        gradient.export_names.contains(&"loss_backward".to_owned()),
        "expected companion name in export_names, got {:?}",
        gradient.export_names
    );

    let interface = radix::program::file_interface::extract_file_interface_with_identity(
        &gradient.analysis,
        &gradient.export_names,
        "gradient",
        None,
    )
    .expect("extract file interface");

    let export = interface
        .exports
        .get("loss_backward")
        .expect("companion export present in interface");
    let FileExportKind::Function(callable) = &export.kind else {
        panic!(
            "expected function export for companion, got {:?}",
            export.kind
        );
    };
    // Companion ABI: (tensor x, tensor w, vacuum residual, f32 upstream) → tuple.
    assert_eq!(
        callable.params.len(),
        4,
        "companion params: {:?}",
        callable.params
    );
    assert!(
        matches!(
            callable.ret.as_ref(),
            radix::module::file_interface::InterfaceTypeSnapshot::Tuple(_)
        ),
        "companion return should snapshot as a tuple, got {:?}",
        callable.ret
    );
}

#[cfg(feature = "device")]
#[test]
fn package_mir_runs_library_imported_companion_end_to_end() {
    // SEM004 + LIB-MIR seam: a consumer importing `testgrad:gradient` calls
    // the library's exported `@ radix backward` companion across the import
    // boundary with no mirrored annotation, and the fmir run executes it.
    let dir = test_temp_dir("package-mir-gradus-seam");
    let lib_root = dir.join("lib");
    fs::create_dir_all(lib_root.join("testgrad/src")).expect("create library");
    fs::write(
        lib_root.join("testgrad/faber.toml"),
        r#"[package]
name = "testgrad"
version = "0.1.0"
edition = "2026"

[library]
provider = "testgrad"

[paths]
source = "src"

[build]
kind = "lib"
target = "fmir"
targets = ["fmir"]
[reader]
locale = "la"
"#,
    )
    .expect("write library manifest");
    fs::write(
        lib_root.join("testgrad/src/gradient.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio nil() → vacuum {
    tacet
}

@ publica
@ radix lane "air"
@ radix backward "loss_backward"
functio simple_loss(
    tensor<f32, [2,2]> x,
    tensor<f32, [2,2]> w
) → f32 {
    sit tensor<f32, [2,2]> product ← x ⊙ w
    redde product.media()
}
"#,
    )
    .expect("write library module");

    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "testgrad:gradient" gradient

main {
    let list<f32> data ← [1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [2,2]> x ← data ↦ tensor<f32, [2,2]>
    let tensor<f32, [2,2]> w ← data ↦ tensor<f32, [2,2]>
    let f32 loss ← gradient.simple_loss(x, w)
    print loss
    let f32 upstream ← 1.0
    let _ grads ← gradient.loss_backward(x, w, gradient.nil(), upstream)
    let [_, gw] ← grads
    print gw
}
"#,
    )
    .expect("write entry");

    let config = radix::module::driver::Config::default()
        .with_target(radix::module::codegen::Target::MirFmirBinary)
        .with_library_home(lib_root);
    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&config, &entry, &mut host);

    assert!(
        result.is_ok(),
        "library-imported companion should execute, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec!["7.5".to_owned(), "[0.25, 0.5, 0.75, 1.0]".to_owned()]
    );
}

#[test]
fn package_mir_linking_fails_closed_on_unresolvable_companion() {
    // codex-gap S1 U4 s1-companion-fail-closed: a declared `@ radix backward`
    // companion with no resolvable symbol in the library analysis fails
    // closed at link time with the stable `package_mir_companion_unresolved`
    // identity instead of silently skipping the companion (which previously
    // surfaced as an unstable runner-internal `missing MIR function` error).
    // The primal here has no tensor parameter, so radix's reverse-AD snapshot
    // pass skips it and no companion function is ever generated.
    let dir = test_temp_dir("package-mir-companion-unresolved");
    let lib_root = dir.join("lib");
    fs::create_dir_all(lib_root.join("testgrad/src")).expect("create library");
    fs::write(
        lib_root.join("testgrad/faber.toml"),
        r#"[package]
name = "testgrad"
version = "0.1.0"
edition = "2026"

[library]
provider = "testgrad"

[paths]
source = "src"

[build]
kind = "lib"
target = "fmir"
targets = ["fmir"]
[reader]
locale = "la"
"#,
    )
    .expect("write library manifest");
    fs::write(
        lib_root.join("testgrad/src/gradient.fab"),
        r#"+++
locale = "la"
+++
@ publica
functio nil() → vacuum {
    tacet
}

@ publica
@ radix lane "air"
@ radix backward "ghost"
functio simple_add(i32 a, i32 b) → numerus {
    redde a + b
}
"#,
    )
    .expect("write library module");

    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "testgrad:gradient" gradient

main {
    let _ g ← gradient.ghost(gradient.nil(), 5)
    print "companion-ran"
}
"#,
    )
    .expect("write entry");

    let config = radix::module::driver::Config::default()
        .with_target(radix::module::codegen::Target::MirFmirBinary)
        .with_library_home(lib_root);
    let mut host = BufferHost::default();
    let diagnostics = run_mir_for_faber(&config, &entry, &mut host)
        .expect_err("unresolvable companion should fail closed at link time");

    assert_eq!(host.stdout_lines, [] as [String; 0]);
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "package_mir_companion_unresolved")
                && diagnostic_has_arg(diag, "companion", "ghost")
                && diagnostic_has_arg(diag, "import", "testgrad:gradient")
        }),
        "expected package_mir_companion_unresolved diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_mir_executes_library_function_that_calls_nested_library() {
    let dir = test_temp_dir("package-mir-nested-library-call");
    let lib_root = dir.join("lib");
    fs::create_dir_all(lib_root.join("testlib/src")).expect("create library");
    fs::write(
        lib_root.join("testlib/faber.toml"),
        r#"[package]
name = "testlib"
version = "0.1.0"
edition = "2026"

[library]
provider = "testlib"

[paths]
source = "src"

[build]
kind = "lib"
target = "fmir"
targets = ["fmir"]
[reader]
locale = "la"
"#,
    )
    .expect("write library manifest");
    fs::write(
        lib_root.join("testlib/src/inner.fab"),
        r#"+++
locale = "la"
+++
@ interna
functio dupla(numerus value) → numerus {
    redde value * 2
}
"#,
    )
    .expect("write nested library module");
    fs::write(
        lib_root.join("testlib/src/outer.fab"),
        r#"+++
locale = "la"
+++
importa ex "testlib:inner" inner

@ publica
functio computa(numerus value) → numerus {
    redde inner.dupla(value) + 1
}
"#,
    )
    .expect("write importing library module");

    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"import from "testlib:outer" outer

main {
    print outer.computa(4)
}
"#,
    )
    .expect("write entry");

    let config = Config::default()
        .with_target(Target::MirFmirBinary)
        .with_library_home(lib_root);
    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&config, &entry, &mut host);

    assert!(
        result.is_ok(),
        "nested library call should link and execute, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, vec!["9".to_owned()]);
}

#[cfg(feature = "device")]
#[test]
fn package_mir_runs_library_imported_companion_with_sub_companion() {
    // LIB-MIR blocker regression: a library primal that calls an unannotated
    // AIR-lane function generates an auto-named `__backward_callee_N`
    // sub-companion interned only into the lowered interner. The library
    // lowering must remap symbols from the post-lowering interner (no
    // out-of-bounds panic) and give the sub-companion a synthetic source.
    let dir = test_temp_dir("package-mir-gradus-subcompanion");
    let lib_root = dir.join("lib");
    fs::create_dir_all(lib_root.join("testgrad/src")).expect("create library");
    fs::write(
        lib_root.join("testgrad/faber.toml"),
        r#"[package]
name = "testgrad"
version = "0.1.0"
edition = "2026"

[library]
provider = "testgrad"

[paths]
source = "src"

[build]
kind = "lib"
target = "fmir"
targets = ["fmir"]
[reader]
locale = "la"
"#,
    )
    .expect("write library manifest");
    fs::write(
        lib_root.join("testgrad/src/gradient.fab"),
        r#"+++
locale = "la"
+++
@ radix lane "air"
functio inner(tensor<f32, [2,2]> a, tensor<f32, [2,2]> b) → tensor<f32, [2,2]> {
    redde a + b
}

@ publica
functio nil() → vacuum {
    tacet
}

@ publica
@ radix lane "air"
@ radix backward "loss_backward"
functio simple_loss(
    tensor<f32, [2,2]> x,
    tensor<f32, [2,2]> w
) → f32 {
    sit tensor<f32, [2,2]> product ← x ⊙ inner(x, w)
    redde product.media()
}
"#,
    )
    .expect("write library module");

    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "testgrad:gradient" gradient

main {
    let list<f32> data ← [1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [2,2]> x ← data ↦ tensor<f32, [2,2]>
    let tensor<f32, [2,2]> w ← data ↦ tensor<f32, [2,2]>
    let f32 loss ← gradient.simple_loss(x, w)
    print loss
    let f32 upstream ← 1.0
    let _ grads ← gradient.loss_backward(x, w, gradient.nil(), upstream)
    let [_, gw] ← grads
    print gw
}
"#,
    )
    .expect("write entry");

    let config = radix::module::driver::Config::default()
        .with_target(radix::module::codegen::Target::MirFmirBinary)
        .with_library_home(lib_root);
    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&config, &entry, &mut host);

    assert!(
        result.is_ok(),
        "library-imported companion with sub-companion should execute, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec!["15.0".to_owned(), "[0.25, 0.5, 0.75, 1.0]".to_owned()]
    );
}

#[cfg(feature = "device")]
#[test]
fn package_mir_runs_local_helper_backward_through_all_fmir_images() {
    // GT01/E06: a backward-annotated function calling an unannotated local AIR
    // helper generates an auto-named `__backward_callee_N` companion. Its name
    // must survive the validated-context interner into every FMIR image format
    // (source-built, text, binary), and the saved local-helper package must
    // print the same numerical outputs as the inline model.
    let dir = test_temp_dir("package-mir-local-helper");
    fs::create_dir_all(dir.join("src")).expect("create source");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "local-helper"
version = "0.1.0"
edition = "2026"
[paths]
source = "src"
entry = "main.fab"
[build]
target = "fmir"
kind = "bin"
"#,
    )
    .expect("write manifest");
    fs::write(
        dir.join("src/main.fab"),
        r#"+++
locale = "en"
+++
fn nil() → void { pass }

@ radix { lane = "air" }
@ radix backward "user_linear_backward"
fn user_linear(tensor<f32, [2, 2]> x, tensor<f32, [2, 3]> w, tensor<f32, [3]> b, tensor<f32, [2, 3]> target) → f32 {
    let _ y ← project_layer(x, w, b)
    let _ r ← y - target
    return (r ⊙ r).mean()
}

main {
    let list<f32> x_data ← [1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [2, 2]> x ← x_data ↦ tensor<f32, [2, 2]>
    let list<f32> w_data ← [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
    let tensor<f32, [2, 3]> w ← w_data ↦ tensor<f32, [2, 3]>
    let list<f32> b_data ← [0.1, -0.2, 0.3]
    let tensor<f32, [3]> b ← b_data ↦ tensor<f32, [3]>
    let list<f32> target_data ← [1.0, 0.0, 1.0, 0.0, 1.0, 0.0]
    let tensor<f32, [2, 3]> target ← target_data ↦ tensor<f32, [2, 3]>
    print user_linear(x, w, b, target)
    let _ gs ← user_linear_backward(x, w, b, target, nil(), 1.0)
    print gs[1] ↦ list<f32>
    print gs[2] ↦ list<f32>
}

@ radix { lane = "air" }
fn project_layer(tensor<f32, [2, 2]> x, tensor<f32, [2, 3]> w, tensor<f32, [3]> b) → tensor<f32, [2, 3]> {
    return (x · w).added_bias(b)
}
"#,
    )
    .expect("write entry");
    let entry = dir.join("src/main.fab");

    let expected = vec![
        "3.426667".to_owned(),
        "[2.0, 1.7333333, 3.866667, 2.6666667, 2.5333333, 5.333334]".to_owned(),
        "[0.6666667, 0.8, 1.4666668]".to_owned(),
    ];

    // 1. Source-built image execution.
    let config = Config::default().with_target(Target::MirFmirBinary);
    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&config, &entry, &mut host);
    assert!(
        result.is_ok(),
        "local-helper backward package should execute from source, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, expected);

    // 2. fmir-text image round-trip.
    let image = build_fmir_text_image_for_faber(
        &Config::default().with_target(Target::MirFmir),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let mut host = BufferHost::default();
    let result = run_fmir_text_image_for_faber(&image, &mut host);
    assert!(
        result.is_ok(),
        "local-helper backward package should execute from a fmir-text image, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, expected);

    // 3. Binary artifact round-trip.
    let artifact = build_mir_artifact_for_faber(&Config::default(), &entry, &[])
        .expect("build package MIR artifact");
    let mut host = BufferHost::default();
    let result = run_mir_artifact_for_faber(&Config::default(), &artifact, &mut host);
    assert!(
        result.is_ok(),
        "local-helper backward package should execute from a binary image, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(host.stdout_lines, expected);
}

#[cfg(feature = "device")]
#[test]
fn package_mir_two_library_imports_with_nested_import_executes() {
    // Mirrors the real gradus consumer shape: two imports, the first library
    // itself importing a sibling module.
    let dir = test_temp_dir("package-mir-two-libs");
    let lib_root = dir.join("lib");
    fs::create_dir_all(lib_root.join("testgrad/src")).expect("create library");
    fs::write(
        lib_root.join("testgrad/faber.toml"),
        r#"[package]
name = "testgrad"
version = "0.1.0"
edition = "2026"

[library]
provider = "testgrad"

[paths]
source = "src"

[build]
kind = "lib"
target = "fmir"
targets = ["fmir"]
[reader]
locale = "la"
"#,
    )
    .expect("write library manifest");
    fs::write(
        lib_root.join("testgrad/src/gradient.fab"),
        r#"+++
locale = "la"
+++
importa ex "testgrad:tensor" tensor

@ publica
functio nil() → vacuum {
    tacet
}

@ publica
@ radix lane "air"
@ radix backward "loss_backward"
functio simple_loss(
    tensor<f32, [2,2]> x,
    tensor<f32, [2,2]> w
) → f32 {
    sit tensor<f32, [2,2]> product ← x ⊙ w
    redde product.media()
}
"#,
    )
    .expect("write gradient module");
    fs::write(
        lib_root.join("testgrad/src/tensor.fab"),
        r#"+++
locale = "la"
+++
functio fill(f32 v) → f32 {
    redde v
}
"#,
    )
    .expect("write tensor module");
    fs::write(
        lib_root.join("testgrad/src/math.fab"),
        "+++\nlocale = \"la\"\n+++\nfixum f32 epsilon = 0.00001\n",
    )
    .expect("write math module");

    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
import from "testgrad:gradient" gradient
import from "testgrad:math" math

main {
    let list<f32> data ← [1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [2,2]> x ← data ↦ tensor<f32, [2,2]>
    let tensor<f32, [2,2]> w ← data ↦ tensor<f32, [2,2]>
    let f32 loss ← gradient.simple_loss(x, w)
    print loss
    let f32 upstream ← 1.0
    let _ grads ← gradient.loss_backward(x, w, gradient.nil(), upstream)
    let [_, gw] ← grads
    print gw
}
"#,
    )
    .expect("write entry");

    let config = radix::module::driver::Config::default()
        .with_target(radix::module::codegen::Target::MirFmirBinary)
        .with_library_home(lib_root);
    let mut host = BufferHost::default();
    let result = run_mir_for_faber(&config, &entry, &mut host);

    assert!(
        result.is_ok(),
        "two-library consumer should execute, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec!["7.5".to_owned(), "[0.25, 0.5, 0.75, 1.0]".to_owned()]
    );
}

const WEB4_SPAN_SOURCE: &str = r"@ public
class Span {
    let int low
    let int high
    fn middle() → f64 { return (self.low + self.high) ÷ 2 }
}
";

fn web4_library_package(label: &str) -> TestDir {
    let pkg = test_temp_dir(label);
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("faber.toml"),
        r#"[package]
name = "web4-lib"
version = "0.1.0"
edition = "2026"

[library]
provider = "web4lib"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[locale]
locale = "en"
"#,
    )
    .expect("manifest");
    fs::write(pkg.join("src/span.fab"), WEB4_SPAN_SOURCE).expect("source");
    pkg
}

/// Emit `entry` through the production plan + snapshot path and return the layout.
#[cfg(feature = "hir-rust")]
fn web4_emit_crate(entry: &Path) -> (BuildLayout, super::RustRuntimePlan, String) {
    let plan = runtime_plan_with_faber_seams(&Config::default(), entry).expect("runtime plan");
    let code = compile_package_rust_code(entry);
    let layout = discover_build_layout(entry).expect("layout");
    let meta = layout
        .manifest_path
        .exists()
        .then(|| read_manifest(&layout.manifest_path).expect("manifest"));
    emit_generated_crate_with_runtime_plan(&layout, &code, meta.as_ref(), &plan).expect("emit");
    (layout, plan, code)
}

#[cfg(feature = "hir-rust")]
#[test]
fn web4_lib_kind_package_emits_a_library_crate() {
    let pkg = web4_library_package("web4-lib-kind");
    let (layout, plan, code) = web4_emit_crate(&pkg);
    assert!(
        plan.library_crate,
        "build.kind = \"lib\" plans a library crate"
    );
    let src = layout.generated_crate_root.join("src");
    assert!(src.join("lib.rs").is_file(), "library crate root is lib.rs");
    assert!(!src.join("main.rs").exists(), "no bin root for a library");
    assert!(!code.contains("fn main"), "a library crate carries no main");
    let lib_rs = fs::read_to_string(src.join("lib.rs")).expect("lib.rs");
    assert!(lib_rs.contains("pub struct Span"), "public item is exposed");
    // Exact-integer helpers live in the `faber` crate: the module calls them
    // and carries no copy.
    assert!(
        lib_rs.contains("faber::exact::add(") && !lib_rs.contains("fn __faber_int"),
        "module calls faber::exact"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn web4_manifestless_file_without_entry_point_emits_a_library_crate() {
    let pkg = test_temp_dir("web4-no-main");
    let file = pkg.join("span.fab");
    fs::write(&file, WEB4_SPAN_SOURCE).expect("source");
    let (layout, plan, _code) = web4_emit_crate(&file);
    assert!(
        plan.library_crate,
        "a file with no entry point plans a library"
    );
    let src = layout.generated_crate_root.join("src");
    assert!(src.join("lib.rs").is_file());
    assert!(!src.join("main.rs").exists());
}

#[cfg(feature = "hir-rust")]
#[test]
fn web4_file_with_entry_point_stays_a_binary_crate() {
    let pkg = test_temp_dir("web4-with-main");
    let file = pkg.join("hello.fab");
    fs::write(&file, "main {\n    print \"hi\"\n}\n").expect("source");
    let (layout, plan, _code) = web4_emit_crate(&file);
    assert!(!plan.library_crate);
    let src = layout.generated_crate_root.join("src");
    assert!(src.join("main.rs").is_file());
    assert!(!src.join("lib.rs").exists());
}

#[cfg(feature = "hir-rust")]
#[test]
#[ignore = "builds generated Rust; run with ignored integration checks"]
fn web4_lib_kind_package_cargo_builds_to_an_rlib() {
    let pkg = web4_library_package("web4-lib-cargo");
    let (layout, _plan, _code) = web4_emit_crate(&pkg);
    let artifact = super::invoke_cargo_build_library(&layout, false).expect("cargo build lib");
    assert!(artifact.is_file(), "missing {}", artifact.display());
    assert_eq!(
        artifact.file_name().and_then(|name| name.to_str()),
        Some("libweb4_lib.rlib")
    );
}

#[test]
fn library_import_refuses_nondum_method_and_function_with_sem017() {
    // F1b: an importer of a locked library provider whose
    // interface declares a bodiless `@ unstable` genus method and a bodiless
    // `@ unstable` free function is refused for each call. The importer reads
    // the provider's gates from its extracted file interface.
    let dir = test_temp_dir("library-nondum-method-and-function");
    let lib_root = dir.join("libs");
    let provider = lib_root.join("prov");
    fs::create_dir_all(provider.join("src")).expect("create provider src");
    fs::write(
        provider.join("src/module.fab"),
        r#"+++
locale = "en"
+++
@ public
class Socket {
    mut string host

    @ unstable "pending host dispatch"
    fn send(string data) → void

    fn name() → string {
        return self.host
    }
}

@ public
@ unstable "pending host dispatch"
fn parse_later(string text) → string
"#,
    )
    .expect("write provider module");

    let consumer = dir.join("consumer");
    fs::create_dir_all(consumer.join("src")).expect("create consumer src");
    fs::write(
        consumer.join("faber.toml"),
        r#"[package]
name = "nondum-consumer"
version = "0.1.0"
edition = "2026"

[paths]
entry = "src/main.fab"

[build]
target = "ts"
kind = "bin"

[dependencies]
prov = "0.1.0"
"#,
    )
    .expect("write consumer manifest");
    fs::write(
        consumer.join("faber.lock"),
        format!(
            r#"
[[package]]
name = "prov"
version = "0.1.0"
source = "path"
package_root = "{}"
kind = "lib"
target_language = "ts"
target_triple = "browser"
target_manifest = ""
interface_root = "{}"
artifact = ""
crate = "prov"
rustc = ""
"#,
            provider.display(),
            provider.join("src").display(),
        ),
    )
    .expect("write lock");
    fs::write(
        consumer.join("src/main.fab"),
        r#"import from "prov:module" module

main {
    let module.Socket s ← module.Socket { host = "h" }
    s.send("x")
    print module.parse_later("a")
}
"#,
    )
    .expect("write consumer entry");

    let config = Config::default().with_library_home(lib_root);
    let diagnostics = check_with_faber_seams(&config, &consumer.join("src/main.fab"));
    let refused = diagnostics
        .iter()
        .filter(|diag| diag.issue() == Some("nondum_function_for_target"))
        .count();
    assert_eq!(
        refused,
        2,
        "expected SEM017 for the method call and the function call, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_package_reports_the_locale_an_undeclared_file_reads_as() {
    // `faber check <file>` takes the package route; the default `en` reading
    // of Latin keywords fails, and the advisory names the locale that fits.
    // Latin surface copied from corpus/cape/fac-omnia-dum.fab, no frontmatter.
    let dir = test_temp_dir("undeclared-latin-file");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        "incipit {\n    varia numerus i ← 0\n    fac omnia {\n        i ← i + 1\n        nota \"iteratio §\"(i)\n        si i ≥ 3 {\n            iace \"tertia\"\n        }\n    }\n    cape err {\n        nota \"interceptum\"\n    } dum i ≺ 10\n    nota \"finis §\"(i)\n}\n",
    )
    .expect("write entry");

    let (config, _) =
        config_with_locale(Target::HirRust, &entry, None, None).expect("default config");
    let diagnostics = check_with_faber_seams(&config, &entry);

    let note = diagnostics
        .iter()
        .position(|diag| {
            diag.code == Some("LOCALE002")
                && diagnostic_has_issue(diag, "source_reads_as_other_locale")
                && diagnostic_has_arg(diag, "detected", "la")
        })
        .unwrap_or_else(|| panic!("no detected-locale diagnostic: {diagnostics:?}"));
    let first_error = diagnostics
        .iter()
        .position(Diagnostic::is_error)
        .unwrap_or_else(|| panic!("the wrong-locale read must still fail: {diagnostics:?}"));
    assert!(
        note < first_error,
        "the note must precede the errors: {diagnostics:?}"
    );
    assert!(
        !diagnostics[note].is_error(),
        "detection must not be an error"
    );
}
