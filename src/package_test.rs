//! Package manifest, library resolution, compile/build, and reader-config behavior tests.
//!
//! Rendered diagnostic text contracts live in `package_text_contract_test.rs`.
//! Shared temp-dir and diagnostic arg helpers stay local until a second consumer
//! needs them.

use super::{
    analyze_package, build_package_fmir_image, build_package_fmir_text_image,
    build_package_mir_artifact, check_package, compile_package,
    compile_package_with_test_selection, config_with_reader_locale, discover_build_layout,
    discover_package, emit_generated_crate, emit_generated_crate_with_runtime_plan,
    invoke_cargo_build, library_cached_file_interface, library_resolver_from_config, load_package,
    package_host_selection_diagnostic, package_rust_runtime_plan, read_manifest,
    run_package_fmir_image, run_package_fmir_text_image, run_package_mir, run_package_mir_artifact,
    sanitize_crate_name, use_package_compiler, use_package_compiler_from_args, validate_manifest,
    verify_library_bindings, with_lowered_package_mir, BuildLayout, LibraryInterfaceCache,
    ManifestRustHost,
};
use super::{fmir_image_test_summary, fmir_text_image_test_summary};
use crate::library::{LibraryProviderKind, LibraryResolver, ResolvedLibraryModule};
use radix::codegen::rust::TestSelection;
use radix::codegen::Target;
use radix::diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase};
use radix::driver::Config;
use radix::file_interface::FileExportKind;
use radix::hir::{HirItemKind, LibraryBinding, LibraryItem, LibraryItemKind, LibraryProvider};
use radix::mir::{BufferHost, Host, MirDiagnosticKind, MirProvider, StepperError, Value};
use radix::Output;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn diagnostic_has_issue(diag: &Diagnostic, issue: &str) -> bool {
    diag.args.contains(&DiagnosticArg::new("issue", issue))
}

fn diagnostic_has_arg(diag: &Diagnostic, name: &'static str, value: impl Into<String>) -> bool {
    diag.args.contains(&DiagnosticArg::new(name, value))
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
    fn scribe(&mut self, kind: MirDiagnosticKind, text: &str) -> Result<(), StepperError> {
        self.buffer.scribe(kind, text)
    }

    fn read_line(&mut self) -> Result<Option<String>, StepperError> {
        self.buffer.read_line()
    }

    fn abort(&mut self, reason: &str) -> ! {
        self.buffer.abort(reason)
    }

    fn provider(&mut self, provider: &MirProvider) -> Result<Value, StepperError> {
        self.buffer.provider(provider)
    }

    fn exit(&mut self, code: i32) -> ! {
        std::panic::panic_any(ExitPanic(code));
    }

    fn argumenta(&self) -> &[String] {
        self.buffer.argumenta()
    }
}

fn test_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("radix-project-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn dev_norma_library_home() -> PathBuf {
    if let Some(home) = std::env::var_os("FABER_LIBRARY_HOME")
        .map(PathBuf::from)
        .filter(|path| path.join("norma/src").exists())
    {
        return home;
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("faberlang container root");
    for candidate in workspace.ancestors() {
        if candidate.join("norma/src").exists() {
            return candidate.to_path_buf();
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

struct CoreutilsLikePackage {
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
"#,
    )
    .expect("write manifest");
    CoreutilsLikePackage {
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
    analysis: &'a radix::driver::AnalyzedUnit,
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
    analysis: &'a radix::driver::AnalyzedUnit,
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

fn write_zh_reader_pack(root: &Path, name: &str) -> PathBuf {
    let reader = root.join("reader");
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

[diagnostics.READER001]
message = "{pack} used Latin {keyword}; prefer {localized}"
help = "use {localized}"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("write reader pack");
    pack
}

fn compile_emit_build_run(entry: &Path) -> String {
    let result = compile_package(&Config::default(), entry);
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
    let layout = discover_build_layout(entry).expect("layout");
    emit_generated_crate(&layout, &output.code, None).expect("emit generated crate");
    let binary = invoke_cargo_build(&layout, false).expect("cargo build");
    let run = Command::new(binary).output().expect("run generated binary");
    assert!(run.status.success(), "generated binary failed: {:?}", run);
    String::from_utf8(run.stdout).expect("stdout utf8")
}

#[test]
fn compile_package_reports_unresolved_external_imports() {
    let dir = test_temp_dir("external-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        "importa ex \"lodash\" privata map\nincipit { nota \"x\" }",
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "package_import_unsupported_path")));
}

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
importa ex "norma:json/solve" privata solve ut json_solve

incipit {
  fac {
    fixum json parsed ← json_solve.solve("{}")
    nota parsed
  }
  cape err {
    nota err
  }
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:json package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn solve"));
    assert!(!output.code.contains("norma::json::solve"));
    assert!(!output.code.contains("crate::norma::json"));
}

#[test]
fn compile_package_prefers_locked_norma_interfaces_over_library_home_without_dependency() {
    let dir = test_temp_dir("locked-norma-platform-default");
    let src = dir.join("src");
    let locked_interfaces = dir.join("store").join("norma/0.1.0/interfaces");
    let library_home = dir.join("library-home");
    fs::create_dir_all(&src).expect("create src");
    fs::create_dir_all(locked_interfaces.join("solum")).expect("create locked interfaces");
    fs::create_dir_all(library_home.join("norma/src/solum")).expect("create fallback interfaces");
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
        locked_interfaces.join("solum/path.fab"),
        r#"
functio locked_label() → textus {
  redde "locked"
}
"#,
    )
    .expect("write locked norma interface");
    fs::write(
        library_home.join("norma/src/solum/path.fab"),
        r#"
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
importa ex "norma:solum/path" privata path

incipit {
  nota path.locked_label()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default().with_stdlib(library_home), &entry);
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

#[test]
fn package_emits_rustfmt_clean_generated_main_for_multifile_package() {
    let dir = test_temp_dir("rustfmt-clean-multifile-package");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r##"
importa ex "./thing" privata thing

incipit {
  nota thing.label()
}
"##,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r#"
functio label() → textus {
  redde "ok"
}
"#,
    )
    .expect("write module");

    let result = compile_package(&Config::default(), &entry);
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
fn package_analysis_api_exposes_multifile_units_without_rust_emit() {
    let dir = test_temp_dir("package-analysis-api");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r##"
importa ex "./thing" privata thing

incipit {
  nota thing.label()
}
"##,
    )
    .expect("write entry");
    let module = dir.join("thing.fab");
    fs::write(
        &module,
        r#"
functio label() → textus {
  redde "ok"
}
"#,
    )
    .expect("write module");

    let package = analyze_package(&Config::default(), &entry).expect("analyze package");

    assert!(
        !package.diagnostics.iter().any(|diag| diag.is_error()),
        "expected no analysis errors, got {:?}",
        package
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(package.units.len(), 2);
    let entry_unit = package.entry_unit().expect("entry unit");
    assert_eq!(entry_unit.path, entry);
    assert!(entry_unit.analysis.hir.entry.is_some());
    assert_eq!(
        entry_unit.namespace_exports.get("thing"),
        Some(&vec!["label".to_owned()])
    );

    let module_unit = package
        .units
        .iter()
        .find(|unit| !unit.is_entry)
        .expect("module unit");
    assert_eq!(module_unit.path, module);
    assert_eq!(module_unit.module_segments, vec!["thing".to_owned()]);
    assert!(module_unit.analysis.hir.entry.is_none());
    assert_eq!(module_unit.export_names, vec!["label".to_owned()]);
    assert!(matches!(
        module_unit
            .file_interface
            .exports
            .get("label")
            .map(|export| &export.kind),
        Some(FileExportKind::Function(_))
    ));
}

#[test]
fn package_file_interface_cache_reuses_deterministic_norma_interfaces() {
    let resolver = library_resolver_from_config(&Config::default());
    let mut library_cache = LibraryInterfaceCache::default();

    for (specifier, binding) in [("norma:solum/path", "path"), ("norma:json/solve", "solve")] {
        let module = resolver
            .resolve(specifier)
            .expect("resolve library")
            .expect("builtin module");
        let import = super::LibraryImportBinding {
            binding: binding.to_owned(),
            visibility: radix::syntax::Visibility::Privata,
            import_span: radix::lexer::Span::default(),
            module,
        };

        let first = library_cached_file_interface(&import, &resolver, &mut library_cache)
            .expect("first interface");
        let second = library_cached_file_interface(&import, &resolver, &mut library_cache)
            .expect("cached interface");
        assert_eq!(
            first, second,
            "{specifier} interface changed across cache hits"
        );
        if specifier == "norma:solum/path" {
            assert!(matches!(
                first.exports.get("nomen").map(|export| &export.kind),
                Some(FileExportKind::Function(_))
            ));
        }
    }
}

#[test]
fn package_mir_linking_executes_two_file_package_without_rust_emit() {
    let dir = test_temp_dir("package-mir-linking");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r##"
importa ex "./thing" privata thing

incipit {
  nota thing.label()
}
"##,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r#"
incipit {
  nota "module entry must not run"
}

functio label() → numerus {
  redde 7
}
"#,
    )
    .expect("write module");

    let mut host = BufferHost::default();
    let result = run_package_mir(&Config::default(), &entry, &mut host);

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
incipit argumenta args {
  nota "cli root"
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let result = run_package_mir(&Config::default(), &entry, &mut host);

    assert!(
        result.is_ok(),
        "expected CLI package MIR run success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
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
incipit {
  nota "Salve, Munde!"
}
"#,
    )
    .expect("write entry");

    let artifact = build_package_mir_artifact(&Config::default(), &entry, &[])
        .expect("build package MIR artifact");
    assert!(artifact.root.ends_with("target/faber-mir"));
    assert!(artifact.manifest_path.exists());
    assert!(!dir.join("target/faber/Cargo.toml").exists());

    let mut host = BufferHost::default();
    let result = run_package_mir_artifact(&Config::default(), &artifact, &mut host);

    assert!(
        result.is_ok(),
        "expected package MIR artifact run success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
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
importa ex "norma:solum" privata solum

@ cli "tool"
@ operandus textus root
incipit argumenta args {
  varia lista<textus> partes ← [args.root, "nested"]
  nota solum.iunge(partes)
}
"#,
    )
    .expect("write entry");

    let artifact = build_package_mir_artifact(
        &Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        &[dir.to_string_lossy().into_owned()],
    )
    .expect("build package MIR artifact");
    let manifest = fs::read_to_string(&artifact.manifest_path).expect("read manifest");

    assert!(manifest.contains("[runtime]"), "manifest:\n{manifest}");
    for requirement in [
        "requirement = \"host:argv\"",
        "requirement = \"host:fs\"",
        "requirement = \"host:stdout\"",
        "requirement = \"kernel:solum.iunge\"",
    ] {
        assert!(
            manifest.contains(requirement),
            "missing {requirement} in manifest:\n{manifest}"
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
importa ex "./message" privata message

incipit {
  nota message.text()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("message.fab"),
        r#"
functio text() → textus {
  redde "stable"
}
"#,
    )
    .expect("write module");

    let first = build_package_mir_artifact(&Config::default(), &entry, &[])
        .expect("first package MIR artifact build");
    let first_manifest = fs::read_to_string(&first.manifest_path).expect("read first manifest");
    let second = build_package_mir_artifact(&Config::default(), &entry, &[])
        .expect("second package MIR artifact build");
    let second_manifest = fs::read_to_string(&second.manifest_path).expect("read second manifest");

    assert_eq!(first.manifest_path, dir.join("target/faber-mir/image.toml"));
    assert_eq!(first.manifest_path, second.manifest_path);
    assert_eq!(first_manifest, second_manifest);
    assert_eq!(
        first_manifest,
        r#"version = 2
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

#[derive(Debug)]
struct ArtifactHarnessFailure {
    case: &'static str,
    bucket: &'static str,
    detail: String,
}

struct ArtifactHarnessCase {
    name: &'static str,
    input: PathBuf,
    argumenta: Vec<String>,
    expected_stdout: Vec<String>,
    postcondition: Option<Box<dyn Fn() -> Result<(), String>>>,
}

#[test]
fn package_mir_artifact_harness_initial_floor() {
    let cases = vec![
        artifact_harness_hello_case(),
        artifact_harness_multifile_case(),
        artifact_harness_cli_case(),
        artifact_harness_coreutils_touch_case(),
    ];
    let mut failures = Vec::new();
    for case in cases {
        if let Err(failure) = run_artifact_harness_case(&case) {
            failures.push(failure);
        }
    }
    assert!(
        failures.is_empty(),
        "package artifact harness failures:\n{}",
        format_artifact_harness_failures(&failures)
    );
}

fn format_artifact_harness_failures(failures: &[ArtifactHarnessFailure]) -> String {
    failures
        .iter()
        .map(|failure| format!("{} [{}]: {}", failure.case, failure.bucket, failure.detail))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_artifact_harness_case(case: &ArtifactHarnessCase) -> Result<(), ArtifactHarnessFailure> {
    let config = Config::default().with_stdlib(dev_norma_library_home());
    let artifact = build_package_mir_artifact(&config, &case.input, &case.argumenta).map_err(
        |diagnostics| ArtifactHarnessFailure {
            case: case.name,
            bucket: "build-or-link",
            detail: format!("{:?}", diagnostic_facts(&diagnostics)),
        },
    )?;
    if !artifact.manifest_path.exists() {
        return Err(ArtifactHarnessFailure {
            case: case.name,
            bucket: "image-write",
            detail: artifact.manifest_path.display().to_string(),
        });
    }
    let mut host = BufferHost::with_argumenta(case.argumenta.clone());
    run_package_mir_artifact(&config, &artifact, &mut host).map_err(|diagnostics| {
        ArtifactHarnessFailure {
            case: case.name,
            bucket: "run",
            detail: format!("{:?}", diagnostic_facts(&diagnostics)),
        }
    })?;
    if host.stdout_lines != case.expected_stdout {
        return Err(ArtifactHarnessFailure {
            case: case.name,
            bucket: "output-mismatch",
            detail: format!(
                "expected {:?}, got {:?}",
                case.expected_stdout, host.stdout_lines
            ),
        });
    }
    if let Some(postcondition) = &case.postcondition {
        postcondition().map_err(|detail| ArtifactHarnessFailure {
            case: case.name,
            bucket: "postcondition",
            detail,
        })?;
    }
    Ok(())
}

fn diagnostic_facts(diagnostics: &[Diagnostic]) -> Vec<(Option<&'static str>, Option<&str>)> {
    diagnostics
        .iter()
        .map(|diag| (diag.code, diag.issue()))
        .collect()
}

fn artifact_harness_hello_case() -> ArtifactHarnessCase {
    let dir = test_temp_dir("artifact-harness-hello");
    let input = dir.join("main.fab");
    fs::write(&input, "incipit { nota \"Salve, Munde!\" }").expect("write hello");
    ArtifactHarnessCase {
        name: "hello-world",
        input,
        argumenta: Vec::new(),
        expected_stdout: vec!["Salve, Munde!".to_owned()],
        postcondition: None,
    }
}

fn artifact_harness_multifile_case() -> ArtifactHarnessCase {
    let dir = test_temp_dir("artifact-harness-multifile");
    let input = dir.join("main.fab");
    fs::write(
        &input,
        r#"
importa ex "./message" privata message

incipit {
  nota message.text()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("message.fab"),
        r#"
functio text() → textus {
  redde "multi"
}
"#,
    )
    .expect("write module");
    ArtifactHarnessCase {
        name: "multi-file",
        input,
        argumenta: Vec::new(),
        expected_stdout: vec!["multi".to_owned()],
        postcondition: None,
    }
}

fn artifact_harness_cli_case() -> ArtifactHarnessCase {
    let dir = test_temp_dir("artifact-harness-cli");
    let input = dir.join("main.fab");
    fs::write(
        &input,
        r#"
@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write cli");
    ArtifactHarnessCase {
        name: "cli-argv",
        input,
        argumenta: vec!["Ian".to_owned()],
        expected_stdout: vec!["Ian".to_owned()],
        postcondition: None,
    }
}

fn artifact_harness_coreutils_touch_case() -> ArtifactHarnessCase {
    let dir = test_temp_dir("artifact-harness-coreutils-touch");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create touch src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "touch-artifact-floor"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let touch_source = workspace_root.join("examples/coreutils/packages/touch/src/main.fab");
    fs::write(
        src.join("main.fab"),
        fs::read_to_string(&touch_source).expect("read coreutils touch"),
    )
    .expect("write touch source copy");
    let touched = dir.join("created.txt");
    let touched_for_check = touched.clone();
    ArtifactHarnessCase {
        name: "coreutils-touch",
        input: dir,
        argumenta: vec![touched.to_string_lossy().into_owned()],
        expected_stdout: Vec::new(),
        postcondition: Some(Box::new(move || {
            touched_for_check
                .is_file()
                .then_some(())
                .ok_or_else(|| format!("{} was not created", touched_for_check.display()))
        })),
    }
}

#[test]
fn package_mir_artifact_rejects_malformed_manifest() {
    let dir = test_temp_dir("package-mir-artifact-bad-manifest");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
incipit {
  nota "must not run"
}
"#,
    )
    .expect("write entry");

    let artifact = build_package_mir_artifact(&Config::default(), &entry, &[])
        .expect("build package MIR artifact");
    fs::write(&artifact.manifest_path, "version = 999\n").expect("corrupt manifest");

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir_artifact(&Config::default(), &artifact, &mut host)
        .expect_err("malformed package MIR artifact should fail closed");

    assert!(diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "package_mir_artifact_manifest_metadata_missing")));
    let manifest_file = artifact.manifest_path.to_string_lossy();
    assert!(diagnostics
        .iter()
        .any(|diag| diag.phase == DiagnosticPhase::Tool && diag.file == manifest_file.as_ref()));
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn package_build_scena_target_writes_artifact_without_rust_emit() {
    let dir = test_temp_dir("package-build-scena");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
incipit {
  nota "Salve, Munde!"
}
"#,
    )
    .expect("write entry");

    let config = Config::default().with_target(Target::Scena);
    let artifact =
        build_package_mir_artifact(&config, &entry, &[]).expect("build scena package artifact");

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
incipit {
  nota "Salve, Munde!"
}
"#,
    )
    .expect("write entry");

    let image = build_package_fmir_text_image(
        &Config::default().with_target(Target::FmirText),
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
    run_package_fmir_text_image(&image, &mut host).expect("run fmir-text image without source");

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
importa ex "./message" privata message

incipit {
  nota message.text()
}
"#,
    )
    .expect("write entry");
    fs::write(
        &module,
        r#"
functio text() → textus {
  redde "multi"
}
"#,
    )
    .expect("write module");

    let config = Config::default().with_target(Target::FmirText);
    let first = build_package_fmir_text_image(&config, &entry, &[]).expect("first fmir-text build");
    let first_image = fs::read_to_string(&first.image_path).expect("read first image");
    let second =
        build_package_fmir_text_image(&config, &entry, &[]).expect("second fmir-text build");
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

    fs::write(&module, "functio text() → textus { redde \"changed\" }").expect("mutate module");
    fs::write(&entry, "incipit { nota \"changed\" }").expect("mutate entry");

    let mut host = BufferHost::default();
    run_package_fmir_text_image(&first, &mut host).expect("run fmir-text image after mutation");

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
importa ex "./message" privata message

incipit {
  nota message.text()
}
"#,
    )
    .expect("write entry");
    fs::write(
        &module,
        r#"
functio text() → textus {
  redde "multi"
}
"#,
    )
    .expect("write module");

    let image = build_package_fmir_text_image(
        &Config::default().with_target(Target::FmirText),
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
    fs::write(&entry, "incipit { nota \"quoted\" }").expect("write quoted entry");

    let config = Config::default().with_target(Target::FmirText);
    let artifact = build_package_mir_artifact(&config, &entry, &[]).expect("build manifest");
    let manifest_text = fs::read_to_string(&artifact.manifest_path).expect("read manifest");
    let manifest: toml::Value = toml::from_str(&manifest_text).expect("parse manifest");
    assert_eq!(
        manifest
            .get("entry")
            .and_then(toml::Value::as_str)
            .expect("manifest entry"),
        "main\"quoted.fab"
    );

    let image = build_package_fmir_text_image(&config, &entry, &[]).expect("build fmir-text");
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
    fs::write(&entry, "incipit { nota \"must not run\" }").expect("write entry");

    let image = build_package_fmir_text_image(
        &Config::default().with_target(Target::FmirText),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let mut image_text = fs::read_to_string(&image.image_path).expect("read image");
    image_text = image_text.replacen("version = 2", "version = 999", 1);
    fs::write(&image.image_path, image_text).expect("write corrupt image");
    fs::remove_file(&entry).expect("remove source after corrupting image");

    let mut host = BufferHost::default();
    let diagnostics =
        run_package_fmir_text_image(&image, &mut host).expect_err("bad version should fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_text_image_version_unsupported")
            && diagnostic_has_arg(diag, "actual", "999")
            && diagnostic_has_arg(diag, "expected", "2")
    }));
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn package_fmir_text_image_rejects_unknown_records_without_source_fallback() {
    let dir = test_temp_dir("package-fmir-text-unknown-record");
    let entry = dir.join("main.fab");
    fs::write(&entry, "incipit { nota \"must not run\" }").expect("write entry");

    let image = build_package_fmir_text_image(
        &Config::default().with_target(Target::FmirText),
        &entry,
        &[],
    )
    .expect("build fmir-text image");
    let mut image_text = fs::read_to_string(&image.image_path).expect("read image");
    image_text.push_str("\nunsupported = true\n");
    fs::write(&image.image_path, image_text).expect("write image with unknown record");
    fs::remove_file(&entry).expect("remove source after corrupting image");

    let mut host = BufferHost::default();
    let diagnostics = run_package_fmir_text_image(&image, &mut host)
        .expect_err("unknown fmir-text records should fail closed");

    assert!(diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "fmir_text_image_parse_failed")));
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn package_fmir_text_image_rejects_unknown_runtime_requirement_without_source_fallback() {
    let dir = test_temp_dir("package-fmir-text-unknown-runtime-requirement");
    let entry = dir.join("main.fab");
    fs::write(&entry, "incipit { nota \"must not run\" }").expect("write entry");

    let image = build_package_fmir_text_image(
        &Config::default().with_target(Target::FmirText),
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
    let diagnostics = run_package_fmir_text_image(&image, &mut host)
        .expect_err("unknown fmir-text runtime requirement should fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_runtime_requirement_unsupported")
            && diagnostic_has_arg(diag, "format", "fmir-text")
            && diagnostic_has_arg(diag, "requirement", "host:teleport")
    }));
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn package_fmir_text_image_rejects_bad_type_metadata_without_source_fallback() {
    let dir = test_temp_dir("package-fmir-text-bad-type-metadata");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write entry");

    let image = build_package_fmir_text_image(
        &Config::default().with_target(Target::FmirText),
        &entry,
        &["Ian".to_owned()],
    )
    .expect("build fmir-text image");
    let image_text = fs::read_to_string(&image.image_path).expect("read image");
    let image_text =
        image_text.replacen(r#"{ Primitive = "Textus" }"#, r#"{ Struct = 999999 }"#, 1);
    fs::write(&image.image_path, image_text).expect("write corrupt type metadata");
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::default();
    let diagnostics = run_package_fmir_text_image(&image, &mut host)
        .expect_err("bad type metadata should fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_image_type_metadata_invalid")
            && diagnostic_has_arg(diag, "format", "fmir-text")
    }));
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn package_fmir_text_image_cli_operand_uses_runtime_argument_after_source_is_removed() {
    let dir = test_temp_dir("package-fmir-text-cli-runtime-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write entry");

    let image = build_package_fmir_text_image(
        &Config::default().with_target(Target::FmirText),
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
    run_package_fmir_text_image(&image, &mut host).expect("run CLI fmir-text image");

    assert_eq!(host.stdout_lines, vec!["runtime-value".to_owned()]);
}

#[test]
fn package_fmir_image_cli_operand_uses_runtime_argument_after_source_is_removed() {
    let dir = test_temp_dir("package-fmir-cli-runtime-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write entry");

    let image = build_package_fmir_image(&Config::default().with_target(Target::Fmir), &entry, &[])
        .expect("build fmir image");
    let image_bytes = fs::read(&image.image_path).expect("read image");
    assert_eq!(image.image_path, dir.join("target/faber-mir/image.fmir"));
    assert!(
        !bytes_contain(&image_bytes, b"runtime-value"),
        "image must not bake runtime argv"
    );
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = BufferHost::with_argumenta(vec!["runtime-value".to_owned()]);
    run_package_fmir_image(&image, &mut host).expect("run CLI fmir image");

    assert_eq!(host.stdout_lines, vec!["runtime-value".to_owned()]);
}

#[test]
fn package_fmir_text_image_preserves_fixed_cli_exit_code() {
    let dir = test_temp_dir("package-fmir-text-fixed-exit");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
incipit argumenta args exitus 7 {
  nota "done"
}
"#,
    )
    .expect("write entry");

    let image = build_package_fmir_text_image(
        &Config::default().with_target(Target::FmirText),
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
        run_package_fmir_text_image(&image, &mut host).expect("run fmir-text image");
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
incipit argumenta args exitus 7 {
  nota "done"
}
"#,
    )
    .expect("write entry");

    let image = build_package_fmir_image(&Config::default().with_target(Target::Fmir), &entry, &[])
        .expect("build fmir image");
    let bytes = fs::read(&image.image_path).expect("read image");
    let summary = fmir_image_test_summary(&bytes, &image.image_path).expect("summarize fmir image");
    assert_eq!(summary.exit_code, Some(7));
    fs::remove_file(&entry).expect("remove source after image build");

    let mut host = ExitRecordingHost::default();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_package_fmir_image(&image, &mut host).expect("run fmir image");
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
importa ex "./message" privata message

@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota message.text(args.name)
}
"#,
    )
    .expect("write entry");
    fs::write(
        &module,
        r#"
functio text(textus name) → textus {
  redde "Salve, §!"(name)
}
"#,
    )
    .expect("write module");

    let config = Config::default().with_target(Target::FmirText);
    let text_image =
        build_package_fmir_text_image(&config, &entry, &[]).expect("build fmir-text image");
    let binary_image =
        build_package_fmir_image(&Config::default().with_target(Target::Fmir), &entry, &[])
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
    fs::write(&entry, "incipit { nota \"must not run\" }").expect("write entry");

    let image = build_package_fmir_image(&Config::default().with_target(Target::Fmir), &entry, &[])
        .expect("build fmir image");
    let mut image_bytes = fs::read(&image.image_path).expect("read image");
    image_bytes[0] = 3;
    fs::write(&image.image_path, image_bytes).expect("write corrupt image");
    fs::remove_file(&entry).expect("remove source after corrupting image");

    let mut host = BufferHost::default();
    let diagnostics =
        run_package_fmir_image(&image, &mut host).expect_err("bad fmir version should fail closed");

    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "fmir_image_version_unsupported")
            && diagnostic_has_arg(diag, "actual", "3")
            && diagnostic_has_arg(diag, "expected", "2")
    }));
    assert!(host.stdout_lines.is_empty());
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
@ operandus textus name
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["Ian".to_owned()]);
    let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ operandus textus name
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["Ian".to_owned()]);
    let result = run_package_mir(&Config::default(), &dir, &mut host);

    assert!(
        result.is_ok(),
        "expected manifest CLI operand package MIR success, got {:?}",
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
fn package_mir_callback_receives_validated_norma_program() {
    let dir = test_temp_dir("package-mir-callback");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:solum" privata solum
importa ex "norma:toml" privata toml

incipit {
  solum.scribe("/tmp/faber-package-mir-callback.txt", "salve")
}
"#,
    )
    .expect("write entry");

    let reached = with_lowered_package_mir(
        &Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        |lowered| {
            assert!(!lowered.program.functions.is_empty());
            assert!(lowered.validation.interner.is_some());
            true
        },
    )
    .expect("package MIR callback should receive linked Norma program");

    assert!(reached);
}

#[test]
fn package_mir_json_corpus_emits_verifier_valid_llvm() {
    assert_package_corpus_llvm_smoke("json/json.fab", "package-json-corpus");
}

#[test]
fn package_mir_stage4b_instans_emits_verifier_valid_llvm() {
    assert_package_corpus_llvm_smoke("instans/instans.fab", "package-stage4b-instans");
}

fn assert_package_corpus_llvm_smoke(relative: &str, label: &str) {
    let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/corpus")
        .join(relative);
    let emitted = with_lowered_package_mir(
        &Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        |lowered| {
            let interner = lowered
                .validation
                .interner
                .expect("package MIR carries interner");
            radix::mir::emit_llvm_text_probe_with_context(
                &lowered.program,
                &lowered.validation,
                interner,
            )
        },
    )
    .expect("corpus package lowers to MIR")
    .expect("corpus package emits LLVM");

    if Command::new("llvm-as")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        let llvm_path = test_temp_dir(label).join("module.ll");
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
}

#[test]
fn package_mir_bridges_norma_solum_file_mutation_verbs() {
    let dir = test_temp_dir("package-mir-solum-mutation");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:solum" privata solum

@ cli "tool"
@ operandus textus root
incipit argumenta args {
  varia lista<textus> partes ← [args.root, "nested"]
  fixum textus nested ← solum.iunge(partes)
  solum.crea(nested)
  partes ← [nested, "source.txt"]
  fixum textus source ← solum.iunge(partes)
  partes ← [nested, "copy.txt"]
  fixum textus copy ← solum.iunge(partes)
  partes ← [nested, "moved.txt"]
  fixum textus moved ← solum.iunge(partes)
  solum.tange(source)
  solum.scribe(source, "alpha")
  solum.exscribe(source, copy)
  solum.renomina(copy, moved)
  nota solum.lege<textus>(moved)
  solum.dele(source)
  solum.dele(moved)
  solum.amputa(nested)
  nota solum.exstat(nested)
}
"#,
    )
    .expect("write entry");
    let fixture_root = dir.join("workspace");

    let mut host = BufferHost::with_argumenta(vec![fixture_root.to_string_lossy().into_owned()]);
    let result = run_package_mir(
        &Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        &mut host,
    );

    assert!(
        result.is_ok(),
        "expected norma:solum mutation bridge package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec!["alpha".to_owned(), "falsum".to_owned()]
    );
    assert!(!fixture_root.join("nested").exists());
}

#[test]
fn package_mir_bridges_norma_solum_metadata_and_link_verbs() {
    let dir = test_temp_dir("package-mir-solum-metadata");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:solum" privata solum

@ cli "tool"
@ operandus textus root
incipit argumenta args {
  varia lista<textus> partes ← [args.root, "nested"]
  fixum textus nested ← solum.iunge(partes)
  solum.crea(nested)
  partes ← [nested, "source.bin"]
  fixum textus source ← solum.iunge(partes)
  partes ← [nested, "source-link"]
  fixum textus link ← solum.iunge(partes)
  fixum octeti payload ← |41 42|
  solum.funde(source, payload)
  nota solum.mensura(source)
  nota solum.regularene(source)
  nota solum.directoriumne(nested)
  solum.modum(source, 384)
  nota solum.modus(source)
  solum.vincula(source, link)
  nota solum.vinculumne(link)
  nota solum.sequere(link)
  solum.dele(link)
  solum.dele(source)
  solum.amputa(nested)
}
"#,
    )
    .expect("write entry");
    let fixture_root = dir.join("workspace");
    let source = fixture_root.join("nested").join("source.bin");

    let mut host = BufferHost::with_argumenta(vec![fixture_root.to_string_lossy().into_owned()]);
    let result = run_package_mir(
        &Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        &mut host,
    );

    assert!(
        result.is_ok(),
        "expected norma:solum metadata bridge package MIR success, got {:?}",
        result
            .err()
            .unwrap_or_default()
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        host.stdout_lines,
        vec![
            "2".to_owned(),
            "verum".to_owned(),
            "verum".to_owned(),
            "384".to_owned(),
            "verum".to_owned(),
            source.to_string_lossy().into_owned(),
        ]
    );
    assert!(!fixture_root.join("nested").exists());
}

#[test]
fn package_mir_cli_root_text_operand_preserves_literal_backslashes() {
    let dir = test_temp_dir("package-mir-cli-operand-escape");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operandus textus name
incipit argumenta args {
  nota args.name
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec![r"line\nraw".to_owned()]);
    let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ operandus numerus count
incipit argumenta args {
  nota args.count
}
"#,
    )
    .expect("write entry");

    for value in ["7", "-7"] {
        let mut host = BufferHost::with_argumenta(vec![value.to_owned()]);
        let result = run_package_mir(&Config::default(), &entry, &mut host);

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
fn package_mir_cli_root_rest_text_operand_executes_without_rust_emit() {
    let dir = test_temp_dir("package-mir-cli-rest-text-operand");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
@ cli "tool"
@ operandus textus input
@ operandus ceteri textus files
incipit argumenta args {
  nota args.input
  fixum lista<textus> files ← args.files
  nota files.longitudo()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec![
        "seed".to_owned(),
        "alpha".to_owned(),
        "beta".to_owned(),
    ]);
    let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ operandus lista<numerus> counts
incipit argumenta args {
  fixum lista<numerus> counts ← args.counts
  nota counts.longitudo()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["3".to_owned(), "5".to_owned(), "8".to_owned()]);
    let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ operandus octeti raw
incipit argumenta args {
  fixum octeti raw ← args.raw
  nota raw.longitudo()
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["Az".to_owned()]);
    let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ optio verbose brevis "v" longum "verbose" typus bivalens
incipit argumenta args {
  nota args.verbose
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [
        (vec!["--verbose".to_owned()], "verum"),
        (vec!["--verbose=false".to_owned()], "verum"),
        (vec!["-v".to_owned()], "verum"),
        (Vec::new(), "falsum"),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ optio name brevis "n" longum "name" typus textus vel "worker"
incipit argumenta args {
  nota args.name
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
        let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ optio name longum "name" typus textus
incipit argumenta args {
  nota args.name vel "worker"
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [
        (Vec::new(), "worker"),
        (vec!["--name=Ian".to_owned()], "Ian"),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ optio count longum "count" typus numerus vel 2
incipit argumenta args {
  nota args.count
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
        let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ optio verbose longum "verbose" typus bivalens ubique
incipit argumenta args {
  nota args.verbose
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [
        (Vec::new(), "falsum"),
        (vec!["--verbose".to_owned()], "verum"),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_package_mir(&Config::default(), &entry, &mut host);

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
@ operandus numerus count vel 9
incipit argumenta args {
  nota args.count
}
"#,
    )
    .expect("write entry");

    for (argumenta, expected) in [(Vec::new(), "9"), (vec!["7".to_owned()], "7")] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_package_mir(&Config::default(), &entry, &mut host);

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
incipit argumenta args {
  nota "cli root"
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::with_argumenta(vec!["unexpected".to_owned()]);
    let diagnostics = run_package_mir(&Config::default(), &entry, &mut host)
        .expect_err("extra CLI arguments should not be ignored");

    assert!(host.stdout_lines.is_empty());
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "package_mir_cli_surface_unsupported")
                && diagnostic_has_arg(diag, "surface", "CLI argument parsing")
        }),
        "expected CLI argument parsing diagnostic, got {:?}",
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
incipit argumenta args exitus 7 {
  nota "done"
}
"#,
    )
    .expect("write entry");

    let mut host = ExitRecordingHost::default();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_package_mir(&Config::default(), &entry, &mut host);
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
@ operandus numerus code
incipit argumenta args exitus args.code {
  nota "done"
}
"#,
    )
    .expect("write entry");

    let mut host = ExitRecordingHost::with_argumenta(vec!["7".to_owned()]);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_package_mir(&Config::default(), &entry, &mut host);
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
@ optio count longum "count" typus numerus vel "bad"
incipit argumenta args {
  nota args.count
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir(&Config::default(), &entry, &mut host)
        .expect_err("type-incompatible CLI option defaults should remain unsupported");

    assert!(host.stdout_lines.is_empty());
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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
functio run() argumenta args {
  nota "running"
}
"#,
    )
    .expect("write jobs");

    let mut host = BufferHost::with_argumenta(vec!["jobs".to_owned(), "run".to_owned()]);
    let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
@ operandus textus name
functio run() argumenta args {
  nota args.name
}
"#,
    )
    .expect("write jobs");

    let mut host =
        BufferHost::with_argumenta(vec!["jobs".to_owned(), "run".to_owned(), "Ian".to_owned()]);
    let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
@ operandus numerus count
functio run() argumenta args {
  nota args.count
}
"#,
    )
    .expect("write jobs");

    let mut host =
        BufferHost::with_argumenta(vec!["jobs".to_owned(), "run".to_owned(), "7".to_owned()]);
    let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "sum"
@ operandus ceteri numerus counts
functio sum() argumenta args {
  fixum lista<numerus> counts ← args.counts
  nota counts.longitudo()
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
    let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
@ operandus textus name vel "worker"
functio run() argumenta args {
  nota args.name
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
        let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
@ optio verbose brevis "v" longum "verbose" typus bivalens
functio run() argumenta args {
  nota args.verbose
}
"#,
    )
    .expect("write jobs");

    for (argumenta, expected) in [
        (vec!["jobs".to_owned(), "run".to_owned()], "falsum"),
        (
            vec!["jobs".to_owned(), "run".to_owned(), "--verbose".to_owned()],
            "verum",
        ),
        (
            vec!["jobs".to_owned(), "run".to_owned(), "-v".to_owned()],
            "verum",
        ),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
@ optio name brevis "n" longum "name" typus textus vel "worker"
functio run() argumenta args {
  nota args.name
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
        let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
@ optio name longum "name" typus textus
functio run() argumenta args {
  nota args.name vel "worker"
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
        let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ optio verbose longum "verbose" typus bivalens ubique
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
functio run() argumenta args {
  nota args.verbose
}
"#,
    )
    .expect("write jobs");

    for (argumenta, expected) in [
        (vec!["jobs".to_owned(), "run".to_owned()], "falsum"),
        (
            vec!["--verbose".to_owned(), "jobs".to_owned(), "run".to_owned()],
            "verum",
        ),
    ] {
        let mut host = BufferHost::with_argumenta(argumenta.clone());
        let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ optio name longum "name" typus textus vel "worker" ubique
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
functio run() argumenta args {
  nota args.name
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
        let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ optio name longum "name" typus textus ubique
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
functio run() argumenta args {
  nota args.name vel "worker"
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
        let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ operandus textus tenant ubique
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
@ operandus textus name
functio run() argumenta args {
  nota "§:§"(args.tenant, args.name)
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
    let result = run_package_mir(&Config::default(), &dir, &mut host);

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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
@ alias "start"
@ operandus textus name
functio run() argumenta args {
  nota args.name
}
"#,
    )
    .expect("write jobs");

    let mut host = BufferHost::with_argumenta(vec![
        "jobs".to_owned(),
        "start".to_owned(),
        "Ian".to_owned(),
    ]);
    let result = run_package_mir(&Config::default(), &dir, &mut host);

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
        r##"
importa ex "./thing" privata thing

incipit {
  nota thing.label()
}
"##,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r#"
functio label() → textus {
  redde "ok"
}
"#,
    )
    .expect("write module");

    let mut host = BufferHost::default();
    let result = run_package_mir(&Config::default(), &entry, &mut host);

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
        r##"
importa ex "../../../common/gnu/format" privata gnu_format

@ cli "echo"
@ operandus textus word
incipit argumenta args {
  nota gnu_format.identity(args.word)
}
"##,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("format.fab"),
        r#"
functio identity(textus word) → textus {
  redde word
}
"#,
    )
    .expect("write shared helper");

    let mut host = BufferHost::with_argumenta(vec!["salve".to_owned()]);
    let result = run_package_mir(&Config::default(), &fixture.package, &mut host);

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
        r##"
importa ex "../../../common/gnu/format" privata gnu_format

@ cli "echo"
@ operandus ceteri textus words
incipit argumenta args {
  nota gnu_format.inter_spatia(args.words)
}
"##,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("format.fab"),
        r#"
functio inter_spatia(lista<textus> verba) → textus {
  si verba.longitudo() ≡ 0 ergo redde ""
  varia textus linea ← verba[0]
  varia numerus i ← 1
  dum i < verba.longitudo() {
    linea ← linea + " " + verba[i]
    i ← i + 1
  }
  redde linea
}
"#,
    )
    .expect("write shared formatter");

    let mut host = BufferHost::with_argumenta(vec!["salve".to_owned(), "munde".to_owned()]);
    let result = run_package_mir(&Config::default(), &fixture.package, &mut host);

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
        r##"
importa ex "../../../common/gnu/stdio" privata gnu_stdio

@ cli "echo"
@ operandus textus word
incipit argumenta args {
  gnu_stdio.scribe_identity(args.word)
}
"##,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("stdio.fab"),
        r##"
importa ex "./format" privata format

functio scribe_identity(textus word) → vacuum {
  nota format.identity(word)
}
"##,
    )
    .expect("write shared stdio");
    fs::write(
        fixture.common.join("format.fab"),
        r#"
functio identity(textus word) → textus {
  redde word
}
"#,
    )
    .expect("write shared format");

    let mut host = BufferHost::with_argumenta(vec!["salve".to_owned()]);
    let result = run_package_mir(&Config::default(), &fixture.package, &mut host);

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
        r##"
importa ex "../../../common/gnu/a" privata a

incipit {
  nota a.value()
}
"##,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("a.fab"),
        r##"
importa ex "./b" privata b

functio value() → numerus {
  redde b.value()
}
"##,
    )
    .expect("write shared a");
    fs::write(
        fixture.common.join("b.fab"),
        r##"
importa ex "./a" privata a

functio value() → numerus {
  redde a.value()
}
"##,
    )
    .expect("write shared b");

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir(&Config::default(), &fixture.package, &mut host)
        .expect_err("shared import cycle should fail package analysis");

    assert!(host.stdout_lines.is_empty());
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
        r##"
importa ex "../../../common/gnu/format" privata gnu_format

incipit {
  nota gnu_format.hidden("salve")
}
"##,
    )
    .expect("write entry");
    fs::write(
        fixture.common.join("format.fab"),
        r#"
@ privata
functio hidden(textus word) → textus {
  redde word
}

functio identity(textus word) → textus {
  redde word
}
"#,
    )
    .expect("write shared format");

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir(&Config::default(), &fixture.package, &mut host)
        .expect_err("private shared export should fail package analysis");

    assert!(host.stdout_lines.is_empty());
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "namespace_missing_export")
                && diagnostic_has_arg(diag, "member", "hidden")
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
        r##"
importa ex "./service" privata Service

incipit {
  Service.run()
}
"##,
    )
    .expect("write entry");
    fs::write(
        dir.join("service.fab"),
        r#"
importa ex "./util" privata U

functio run() → vacuum {
  nota U.adde(2, 3)
}
"#,
    )
    .expect("write service");
    fs::write(
        dir.join("util.fab"),
        r#"
functio adde(numerus left, numerus right) → numerus {
  redde left + right
}
"#,
    )
    .expect("write util");

    let mut host = BufferHost::default();
    let result = run_package_mir(&Config::default(), &entry, &mut host);

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
        r##"
importa ex "./a/util" privata A
importa ex "./b/util" privata B

incipit {
  nota A.label()
  nota B.label()
}
"##,
    )
    .expect("write entry");
    fs::write(
        dir.join("a").join("util.fab"),
        r#"
functio label() → numerus {
  redde 1
}
"#,
    )
    .expect("write a util");
    fs::write(
        dir.join("b").join("util.fab"),
        r#"
functio label() → numerus {
  redde 2
}
"#,
    )
    .expect("write b util");

    let mut host = BufferHost::default();
    let result = run_package_mir(&Config::default(), &entry, &mut host);

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
fn package_mir_linking_reports_private_namespace_diagnostic() {
    let dir = test_temp_dir("package-mir-private-namespace");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r##"
importa ex "./auxilium" privata Aux

incipit {
  nota Aux.secretum()
}
"##,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
@ privata
functio secretum() → numerus {
  redde 1
}

functio publicum() → numerus {
  redde 2
}
"#,
    )
    .expect("write auxilium");

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir(&Config::default(), &entry, &mut host)
        .expect_err("private namespace access should fail package analysis");

    assert!(host.stdout_lines.is_empty());
    assert!(
        diagnostics.iter().any(|diag| {
            diagnostic_has_issue(diag, "namespace_missing_export")
                && diagnostic_has_arg(diag, "member", "secretum")
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
        r##"
importa ex "./auxilium" privata Aux

incipit {
  nota Aux.mathesis.dupla(4)
}
"##,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
functio dupla(numerus n) → numerus {
  redde n * 2
}
"#,
    )
    .expect("write auxilium");

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir(&Config::default(), &entry, &mut host)
        .expect_err("nested namespace calls should be explicitly unsupported");

    assert!(host.stdout_lines.is_empty());
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
fn package_mir_linking_reports_norma_imports_as_unsupported_without_execution() {
    let dir = test_temp_dir("package-mir-norma-unsupported");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:chorda" privata chorda

incipit {
  nota chorda.retorta("roma")
}
"#,
    )
    .expect("write entry");

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir(
        &Config::default().with_stdlib(dev_norma_library_home()),
        &entry,
        &mut host,
    )
    .expect_err("norma imports should be explicit unsupported package MIR diagnostics");

    assert!(host.stdout_lines.is_empty());
    assert!(
        diagnostics
            .iter()
            .any(|diag| diagnostic_has_issue(diag, "package_mir_library_imports_unsupported")),
        "expected package MIR library import diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn rust_field_names_default_preserves_source_spelling() {
    let dir = test_temp_dir("rust-field-names-preserve");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
genus PersonaNova {
  textus nomenLongum
}

incipit {
  fixum _ p ← PersonaNova { nomenLongum = "Marcus" }
  nota p.nomenLongum
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
genus PersonaNova {
  textus nomenLongum
}

incipit {
  fixum _ p ← PersonaNova { nomenLongum = "Marcus" }
  nota p.nomenLongum
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
    assert!(!output.code.contains("nomenLongum"));

    let layout = discover_build_layout(&entry).expect("layout");
    emit_generated_crate(&layout, &output.code, None).expect("emit generated crate");
    let clippy = Command::new("cargo")
        .args(["clippy", "--quiet", "--manifest-path"])
        .arg(&layout.generated_cargo_manifest)
        .args(["--target-dir"])
        .arg(&layout.cargo_target_dir)
        .args(["--", "-D", "warnings"])
        .output()
        .expect("cargo clippy");
    assert!(
        clippy.status.success(),
        "snake_case field policy sample must be clippy-clean\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&clippy.stdout),
        String::from_utf8_lossy(&clippy.stderr)
    );
}

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
genus Collidens {
  textus fooBar
  textus foo_bar
}

incipit {
  nota "x"
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
discretio Nuntius {
  Clavis { textus nomenLongum }
}

functio tracta(Nuntius n) → textus {
  discerne n {
    casu Clavis fixum nomenLongum ut captum {
      redde captum
    }
  }
}

incipit {
  fixum Nuntius n ← finge Clavis { nomenLongum = "Julia" } ∷ Nuntius
  nota tracta(n)
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
    assert!(output.code.contains("nomen_longum: captum"));
    assert!(!output.code.contains("nomenLongum"));

    let layout = discover_build_layout(&entry).expect("layout");
    emit_generated_crate(&layout, &output.code, None).expect("emit generated crate");
    let check = Command::new("cargo")
        .args(["check", "--quiet", "--manifest-path"])
        .arg(&layout.generated_cargo_manifest)
        .args(["--target-dir"])
        .arg(&layout.cargo_target_dir)
        .output()
        .expect("cargo check");
    assert!(
        check.status.success(),
        "snake_case variant field policy sample must compile\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
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
    fs::write(&entry, "incipit { nota \"x\" }").expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "invalid_package_manifest")));
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
    assert!(manifest
        .target
        .get("rust")
        .and_then(|target| target.host)
        .is_some());
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
fn compile_package_resolves_builtin_norma_chorda_native_body() {
    let dir = test_temp_dir("norma-chorda-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:chorda" privata chorda

incipit {
  fixum textus reversed ← chorda.retorta("roma")
  nota reversed
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:chorda package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn retorta"));
    assert!(output.code.contains("retorta(\"roma\".to_string())"));
    assert!(output.code.contains("pub mod chorda"));
    assert!(output.code.contains("crate::chorda::retorta"));
    assert!(!output.code.contains("norma::chorda::retorta"));
    assert!(!output.code.contains("panic!(\"unimplemented\")"));
}

#[test]
fn compile_package_resolves_type_only_norma_file_namespace_import() {
    let dir = test_temp_dir("norma-caelum-terminus-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:caelum/terminus" privata terminus

incipit {
  fixum terminus.Terminus endpoint ← terminus.Terminus {
    hospes = "localhost",
    portus = 8080
  }
  nota endpoint.hospes
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:caelum/terminus package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert!(!result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "unused_import")));
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub mod caelum"));
    assert!(output.code.contains("pub mod terminus"));
    assert!(output.code.contains("pub struct Terminus"));
    assert!(output.code.contains(
        "let endpoint: crate::caelum::terminus::Terminus = crate::caelum::terminus::Terminus"
    ));
}

#[test]
fn compile_package_resolves_type_only_local_file_namespace_import() {
    let dir = test_temp_dir("local-type-only-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "./thing" privata Alias

incipit {
  fixum Alias.Thing item ← Alias.Thing { name = "ok" }
  nota item.name
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("thing.fab"),
        r#"
genus Thing {
    textus name
}
"#,
    )
    .expect("write type module");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected local type-only package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    assert!(!result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "unused_import")));
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub mod thing"));
    assert!(output.code.contains("pub struct Thing"));
    assert!(output
        .code
        .contains("let item: crate::thing::Thing = crate::thing::Thing"));
}

#[test]
fn compile_package_preserves_file_stem_named_type_under_import_alias() {
    let library_home = test_temp_dir("alias-stem-type-library_home");
    write_temp_library_fixture(
        &library_home,
        "Thing",
        r#"
genus Thing {
    textus name
}
"#,
    );

    let dir = test_temp_dir("alias-stem-type-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:Thing" privata alias

incipit {
    fixum alias.Thing item ← alias.Thing { name = "ok" }
    nota item.name
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&config_with_library_home(&library_home), &entry);
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
    assert!(output
        .code
        .contains("let item: crate::Thing::Thing = crate::Thing::Thing"));
}

#[test]
fn compile_package_resolves_builtin_norma_chorda_diducta_native_body() {
    let dir = test_temp_dir("norma-chorda-diducta-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:chorda" privata chorda

incipit {
  fac {
    fixum [meta, corpus] ← chorda.diducta("+++\na = 1\n+++\nbody", "+++")
    nota meta, corpus
  } cape err {
    mone err
  }
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:chorda diducta package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn diducta"));
    assert!(output.code.contains("fn _finis_delimitris"));
    assert!(!output.code.contains("norma::chorda::diducta"));
    assert!(!output.code.contains("faber::frame::sermo_open"));
}

#[test]
fn compile_package_chorda_retenta_filters_codegen() {
    let dir = test_temp_dir("norma-chorda-retenta-filters");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:chorda" privata chorda

incipit {
  nota chorda.retenta("hello7!", "a", "z")
  nota chorda.retenta_iudicio("hello7!", textus c ∴ c ≥ 'a' et c ≤ 'z')
  nota chorda.expurgata("hello7!", "0", "9")
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
        output.code.contains("fn retenta("),
        "interval filter should export retenta: {}",
        output.code
    );
    assert!(
        output.code.contains("fn retenta_iudicio("),
        "predicate filter should export retenta_iudicio: {}",
        output.code
    );
    assert!(
        !output.code.contains("Some(|"),
        "predicate overload should not wrap closure in Some: {}",
        output.code
    );

    let layout = discover_build_layout(&entry).expect("layout");
    emit_generated_crate(&layout, &output.code, None).expect("emit generated crate");
    let binary = invoke_cargo_build(&layout, false).expect("cargo build");
    let run = Command::new(binary).output().expect("run generated binary");

    assert!(run.status.success(), "chorda binary failed: {:?}", run);
    let stdout = String::from_utf8(run.stdout).expect("stdout utf8");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["hello", "hello", "hello!"]
    );
}

#[test]
fn compile_package_resolves_builtin_norma_tensor_native_body() {
    let dir = test_temp_dir("norma-tensor-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:tensor" privata tensor

incipit {
  nota "tensor"
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
    assert!(output.code.contains("fn planata"));
    assert!(output.code.contains("fn structa"));
    assert!(output.code.contains("grid.planata()"));
    assert!(!output.code.contains("norma::tensor::planata"));
}

#[test]
fn compile_package_resolves_builtin_norma_chorda_mixed_native_and_runtime_backed() {
    let dir = test_temp_dir("norma-chorda-mixed");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:chorda" privata chorda

incipit {
  fixum octeti raw ← chorda.pange("Salve")
  nota chorda.solve(raw)
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:chorda mixed package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn retorta"));
    assert!(output.code.contains("pub mod chorda"));
    assert!(output.code.contains("crate::chorda::pange"));
    assert!(output.code.contains("crate::chorda::solve"));
    assert!(output.code.contains("pub(crate) fn pange"));
}

#[test]
fn compile_package_resolves_builtin_norma_toml_library_imports() {
    let dir = test_temp_dir("norma-toml-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:toml" privata toml

incipit {
  fixum _ parsed ← toml.solve("name = \"faber\"")
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:toml package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("crate::toml::solve"));
    assert!(output.code.contains("pub(crate) fn solve"));
    assert!(!output.code.contains("norma::toml::solve"));
}

#[test]
fn compile_package_resolves_builtin_norma_toml_native_navigation_body() {
    let dir = test_temp_dir("norma-toml-native-nav");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:toml" privata toml
importa ex "norma:valor" privata valor

incipit {
  fixum valor doc ← toml.solve("id = \"sample\"")
  fac {
    fixum textus id ← valor.exige(doc, "id")
    nota id
  } cape err {
    mone err
  }
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:toml native navigation compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn cape"));
    assert!(output.code.contains("fn exige"));
    assert!(!output.code.contains("norma::toml::cape"));
    assert!(!output.code.contains("norma::toml::exige"));
}

#[test]
fn compile_package_resolves_builtin_norma_toml_exige_claves_native_body() {
    let dir = test_temp_dir("norma-toml-exige-claves");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:toml" privata toml
importa ex "norma:valor" privata valor

incipit {
  fixum valor doc ← toml.solve("id = \"sample\"")
  fac {
    valor.exige_claves(doc, ["id", "name"])
    nota "ok"
  } cape err {
    mone err
  }
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:toml exige_claves compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn exige_claves"));
    assert!(!output.code.contains("norma::toml::exige_claves"));
}

#[test]
fn compile_package_resolves_builtin_norma_solum_explora_contract() {
    let dir = test_temp_dir("norma-solum-explora-contract");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:solum" privata solum

incipit {
  nota "solum"
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:solum explora contract compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("fn explora"));
    assert!(!output.code.contains("norma::solum::explora"));
}

#[test]
fn compile_package_resolves_builtin_norma_consolum_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-consolum-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:consolum" privata consolum

incipit {
  consolum.dic("salve")
  fixum _ tty ← consolum.audit()
  consolum.funde(|48 65 6c 6c 6f|)
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:consolum package compile success, got {:?}",
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
    assert!(output.code.contains("fn dic"));
    assert!(output.code.contains("crate::consolum::funde"));
    assert!(output.code.contains("pub(crate) fn funde"));
    assert!(!output.code.contains("norma::consolum::funde"));
    assert!(!output.code.contains("norma::consolum::dic"));
    assert!(!output.code.contains("norma::consolum::audit"));
    assert!(!output.code.contains("crate::norma::hal::consolum"));
}

#[test]
fn compile_package_resolves_builtin_norma_solum_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-solum-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:solum" privata solum ut terra

incipit {
  fixum _ exists ← terra.exstat(".")
  fixum _ parent ← terra.parens("a/b")
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:solum package compile success, got {:?}",
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
    assert!(output.code.contains("faber::frame::sermo_open"));
    assert!(output.code.contains("fn exstat"));
    assert!(!output.code.contains("norma::solum::exstat"));
    assert!(!output.code.contains("norma::solum::parens"));
    assert!(!output.code.contains("crate::norma::hal::solum"));
}

#[test]
fn compile_package_resolves_builtin_norma_processus_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-processus-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:processus" privata processus ut proc

incipit {
  fixum _ out ← proc.exsequi("echo hi")
  fixum _ env ← proc.lege("PATH")
  fixum _ _ ← proc.genera(["true"])
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:processus package compile success, got {:?}",
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
    assert!(output.code.contains("fn exsequi"));
    assert!(output.code.contains("crate::processus::genera"));
    assert!(output.code.contains("pub(crate) fn genera"));
    assert!(!output.code.contains("norma::processus::genera"));
    assert!(!output.code.contains("norma::processus::exsequi"));
    assert!(!output.code.contains("norma::processus::lege"));
    assert!(!output.code.contains("crate::norma::hal::processus"));
}

#[test]
fn compile_package_resolves_builtin_norma_tempus_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-tempus-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:tempus" privata tempus

incipit {
  fixum _ now ← tempus.nunc()
  fixum _ mono ← tempus.monotonicum()
  fixum _ active ← tempus.activum()
  fixum _ ms ← tempus.MILLISECUNDUM()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:tempus package compile success, got {:?}",
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
    assert!(output.code.contains("fn nunc"));
    assert!(!output.code.contains("norma::tempus::nunc"));
    assert!(!output.code.contains("norma::tempus::monotonicum"));
    assert!(!output.code.contains("norma::tempus::millisecundum"));
    assert!(!output.code.contains("crate::norma::hal::tempus"));
}

#[test]
fn compile_package_resolves_builtin_norma_aleator_ad_wrapped_imports() {
    let dir = test_temp_dir("norma-aleator-ad-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:aleator" privata aleator

incipit {
  fixum _ f ← aleator.fractum()
  fixum _ n ← aleator.sortire(1, 6)
  fixum _ id ← aleator.uuid()
  fixum _ bytes ← aleator.octetos(8)
  aleator.semina(42)
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:aleator package compile success, got {:?}",
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
    assert!(output.code.contains("fn fractum"));
    assert!(!output.code.contains("norma::aleator::fractum"));
    assert!(!output.code.contains("norma::aleator::sortire"));
    assert!(!output.code.contains("norma::aleator::uuid"));
    assert!(!output.code.contains("crate::norma::hal::aleator"));
}

#[test]
fn compile_package_resolves_builtin_norma_yaml_imports() {
    let dir = test_temp_dir("norma-yaml-import");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:yaml" privata yaml

incipit {
  fixum valor doc ← yaml.solve("count: 1")
  fixum _ text ← yaml.pange(doc)
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected norma:yaml package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("crate::yaml::solve"));
    assert!(output.code.contains("crate::yaml::pange"));
    assert!(
        output.code.contains("crate::yaml::solve")
            && (output.code.contains("faber::Valor")
                || output.code.contains("use faber::Valor as valor;")
                || output.code.contains(": valor")),
        "expected valor/yaml binding via faber::Valor, got:\n{}",
        output.code
    );
    assert!(
        !output.code.contains(&["Faber", "Value"].concat()),
        "generated code must not reference legacy valor type:\n{}",
        output.code
    );
}

#[test]
fn compile_package_rejects_removed_norma_tempus_method_calls() {
    let dir = test_temp_dir("norma-tempus-removed-method");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:tempus" privata tempus

incipit {
  tempus.siste(1)
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);

    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| diag.is_error()
        && diagnostic_has_issue(diag, "namespace_missing_export")
        && diagnostic_has_arg(diag, "member", "siste")
        && diag
            .source_line
            .as_deref()
            .is_some_and(|line| line.contains("siste"))));
}

#[test]
fn compile_package_rejects_removed_norma_vector_placeholder_calls() {
    for (method, args) in [("crea", "[0]"), ("shuffle", "nihil, [0]")] {
        let dir = test_temp_dir(&format!("norma-vector-removed-{method}"));
        let entry = dir.join("main.fab");
        fs::write(
            &entry,
            format!(
                r#"
importa ex "norma:vector" privata vector

incipit {{
  vector.{method}({args})
}}
"#
            ),
        )
        .expect("write entry");

        let result = compile_package(&Config::default(), &entry);

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
importa ex "norma:http" privata http

incipiet {
    fixum _ responsum ← cede http.petet("http://127.0.0.1:9/test")
    nota responsum.status()
    fixum _ caput ← responsum.caput("x-faber-test")
    si caput est nihil { nota "header:missing" } secus { nota "header:present" }
    nota responsum.corpus()
    fixum _ data ← responsum.corpus_json()
    si data.is_nihil() { nota "json:missing" } secus { nota "json:present" }
}
"#,
    )
    .expect("entry");

    let layout = discover_build_layout(&dir).expect("layout");
    let compile_result = compile_package(&Config::default(), &dir);
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
    assert!(output.code.contains("crate::http::petet"));
    assert!(output
        .code
        .contains("tokio::runtime::Builder::new_current_thread"));
    assert!(output.code.contains(".corpus_json()"));
    assert!(
        output.code.contains(".corpus_json()")
            && (output.code.contains("faber::Valor")
                || output.code.contains("use faber::Valor as valor;")
                || output.code.contains(": valor")),
        "expected corpus_json valor binding via faber::Valor, got:\n{}",
        output.code
    );
    assert!(
        !output.code.contains(&["Faber", "Value"].concat()),
        "generated code must not reference legacy valor type:\n{}",
        output.code
    );
    assert!(
        output
            .code
            .contains("norma:http.petet is deferred pending Stage 2 dispatch"),
        "expected deferred native http wire floor, got:\n{}",
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
    let resolved = LibraryResolver::default()
        .resolve("norma:solum")
        .expect("resolve should not fail")
        .expect("norma:solum should resolve");

    assert_eq!(resolved.package, "norma");
    assert_eq!(resolved.module_path, vec!["solum"]);
    assert!(resolved.interface_path.ends_with("norma/src/solum.fab"));
    assert_eq!(resolved.provider, LibraryProviderKind::PackageDependency);
}

#[test]
fn library_resolver_rejects_retired_norma_hal_paths() {
    let err = LibraryResolver::default()
        .resolve("norma:hal/solum")
        .expect_err("norma:hal/solum should be rejected");

    assert!(matches!(
        err,
        crate::library::LibraryResolveError::UnknownModule {
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
importa ex "norma:json/solve" privata solve

incipit {
  solve.nonexistent("{}")
}
"#,
    )
    .expect("write entry");

    let diagnostics = check_package(&Config::default(), &dir.join("faber.toml"));
    assert!(diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "namespace_missing_export")
            && diagnostic_has_arg(diag, "member", "nonexistent")
    }));
}

#[test]
fn compile_package_rejects_faber_kernel_imports() {
    let dir = test_temp_dir("faber-kernel-package");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "faber:json" privata json
incipit {}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "kernel_import_script_mode_only")));
}

#[test]
fn compile_package_reports_unknown_library_modules() {
    let dir = test_temp_dir("norma-nope");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:nope" privata nope
incipit {}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
    assert!(known_modules.contains("solum"));
    assert!(!known_modules.contains("nope"));
}

#[test]
fn compile_package_rejects_old_norma_slash_library_imports() {
    let dir = test_temp_dir("norma-old-slash");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma/http" privata http
incipit {}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "old_builtin_norma_specifier")
            && diagnostic_has_arg(diag, "replacement", "norma:http")
    }));
}

#[test]
fn compile_package_keeps_relative_norma_paths_as_local_imports() {
    let dir = test_temp_dir("relative-norma-path");
    fs::create_dir_all(dir.join("norma")).expect("norma dir");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "./norma/json" privata local
incipit {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("norma/json.fab"),
        r#"
functio salve() → textus {
    redde "salve"
}
"#,
    )
    .expect("write local module");

    let result = compile_package(&Config::default(), &entry);
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
importa ex "sqlite:client" privata client
incipit {}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
    assert!(!result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "package_import_unsupported_path")));
}

#[test]
fn dual_norma_format_imports_scope_provenance_by_binding() {
    let dir = test_temp_dir("norma-json-yaml-provenance");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:yaml" privata yaml
importa ex "norma:json" privata json

incipit {
  fixum valor yaml_doc ← yaml.solve("count: 1")
  fixum _ yaml_text ← yaml.pange(yaml_doc)
  fac {
    fixum json json_doc ← json.solve("{\"count\": 1}")
    fixum _ json_text ← json.pange(json_doc)
  }
  cape err {
    nota err
  }
}

"#,
    )
    .expect("write entry");

    let package = analyze_package(&Config::default(), &entry).expect("analyze package");
    let analysis = &package.entry_unit().expect("entry unit").analysis;

    let yaml_item = library_item_by_export(analysis, &["yaml"], "pange", LibraryItemKind::Function);
    let json_item = library_item_by_export(analysis, &["json"], "pange", LibraryItemKind::Function);
    assert_ne!(
        yaml_item.def_id, json_item.def_id,
        "overlapping function names must keep distinct DefIds"
    );
    assert_eq!(yaml_item.identity.module_path, vec!["yaml"]);
    assert_eq!(json_item.identity.module_path, vec!["json"]);
    assert_eq!(yaml_item.exported_name, "pange");
    assert_eq!(json_item.exported_name, "pange");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected json+yaml package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("crate::yaml::pange"));
    assert!(output.code.contains("crate::yaml::solve"));
    assert!(output.code.contains("pub(crate) fn pange"));
    assert!(output.code.contains("pub(crate) fn solve"));
    assert!(!output.code.contains("norma::yaml::pange"));
    assert!(!output.code.contains("norma::yaml::solve"));
    assert!(!output.code.contains("norma::json::pange"));
    assert!(!output.code.contains("norma::json::solve"));
}

#[test]
fn compile_package_preserves_norma_runtime_types_and_failable_calls() {
    let dir = test_temp_dir("norma-runtime-type-and-failable-calls");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:json" privata json
importa ex "norma:valor" privata valor

incipit {
    fixum json doc ← { "wire_name": "Ada" }
    fixum textus wire ← json.pange(doc)
    fixum valor tree ← { "wire_name": "Ada" } ↦ valor
    fixum valor child ← valor.cape(tree, "wire_name")
    fac {
        fixum json parsed ← json.solve(wire)
        nota parsed, child
    }
    cape err {
        nota err
    }
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
    assert!(output.code.contains("let tree: faber::Valor"));
    assert!(output
        .code
        .contains("match crate::json::solve(wire.clone())"));
    assert!(output
        .code
        .contains("crate::chorda::discissa(via.clone(), \".\".to_string()"));
}

#[test]
fn aliased_norma_import_preserves_provider_identity_in_analysis() {
    let dir = test_temp_dir("aliased-norma-provider");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:http" privata http ut rete

incipit {
  fixum _ responsum ← cede rete.petet("http://127.0.0.1:9")
}
"#,
    )
    .expect("write entry");

    let package = analyze_package(&Config::default(), &entry).expect("analyze package");
    let analysis = &package.entry_unit().expect("entry unit").analysis;

    let binding = library_binding_by_module(analysis, &["http"]);
    assert_eq!(
        binding.identity.provider,
        LibraryProvider::Builtin("norma".to_owned())
    );
    assert_eq!(binding.identity.module_path, vec!["http"]);
    assert!(binding.rust_runtime_module.is_none());

    assert!(
        analysis.hir.items.iter().all(|item| {
            let HirItemKind::Interface(interface) = &item.kind else {
                return true;
            };
            analysis.interner.resolve(interface.name) != "Replicatio"
        }),
        "library interfaces should not be source-spliced into importer HIR"
    );
    let item = library_item_by_export(
        analysis,
        &["http"],
        "Replicatio",
        LibraryItemKind::Interface,
    );
    assert_eq!(
        item.identity.provider,
        LibraryProvider::Builtin("norma".to_owned())
    );
    assert_eq!(item.identity.module_path, vec!["http"]);
    assert_eq!(item.exported_name, "Replicatio");
    assert_eq!(item.kind, LibraryItemKind::Interface);
    assert!(item.rust_runtime_type.is_none());
    assert!(!item.elide_rust_decl);
}

#[test]
fn aliased_norma_http_import_lowers_by_provider_identity() {
    let dir = test_temp_dir("aliased-norma-http-lowering");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:http" privata http ut rete

incipiet {
  fixum _ responsum ← cede rete.petet("http://127.0.0.1:9")
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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

    assert!(output
        .code
        .contains("crate::http::petet(\"http://127.0.0.1:9\".to_string()).await"));
    assert!(output
        .code
        .contains("let responsum: dyn crate::http::Replicatio ="));
    assert!(!output.code.contains("rete.petet"));
    assert!(output.code.contains("pub trait Replicatio"));
}

#[test]
fn resolved_library_module_shape_can_describe_future_sqlite_without_rust_metadata() {
    let module = ResolvedLibraryModule::new(
        "sqlite",
        vec!["transactio".to_owned()],
        "/tmp/faber-libs/sqlite/transactio.fab",
        LibraryProviderKind::PackageDependency,
    );

    assert_eq!(module.package, "sqlite");
    assert_eq!(module.module_path, vec!["transactio"]);
    assert_eq!(module.module_name(), Some("transactio"));
    assert_eq!(module.provider, LibraryProviderKind::PackageDependency);
    assert!(module.interface_path.ends_with("sqlite/transactio.fab"));
}

#[test]
fn compile_package_resolves_relative_input_from_current_working_directory() {
    let dir = test_temp_dir("relative-input");
    let project_dir = dir.join("project");
    fs::create_dir_all(&project_dir).expect("create project dir");
    fs::write(project_dir.join("main.fab"), "incipit { nota \"salve\" }").expect("write entry");

    let original_cwd = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&dir).expect("set current dir");

    let result = compile_package(&Config::default(), Path::new("./project/main.fab"));

    std::env::set_current_dir(original_cwd).expect("restore current dir");

    assert!(
        result.success(),
        "expected relative package compile success"
    );
}

#[test]
fn compile_package_mounts_wildcard_imported_cli_commands() {
    let dir = test_temp_dir("cli-mount");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "config/set"
@ alias "set"
@ operandus textus name
functio set_config() argumenta args {
  nota args.name
}
"#,
    )
    .expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
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
    assert!(output
        .code
        .contains("pub(crate) fn set_config(args: crate::CliArgsJobsConfigSet)"));
    assert!(output.code.contains("jobs::set_config(args);"));
    assert!(output.code.contains("Usage: tool jobs config set"));
    assert!(output
        .code
        .contains("command_parts[0] == \"jobs\" && command_parts[1] == \"set\""));
}

#[test]
fn check_package_validates_mounted_cli_commands_without_emitting() {
    let dir = test_temp_dir("check-cli-mount");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
functio run() argumenta args {
  nota "running"
}
"#,
    )
    .expect("write jobs");

    let diagnostics = check_package(&Config::default(), &dir);

    assert!(
        !diagnostics.iter().any(Diagnostic::is_error),
        "expected package check success, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn compile_package_mounted_handlers_can_access_root_globals() {
    let dir = test_temp_dir("cli-mount-root-global");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ optio verbose longum "verbose" typus bivalens ubique
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
functio run() argumenta args {
  nota args.verbose
}
"#,
    )
    .expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
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
    assert!(output
        .code
        .contains("println!(\"{}\", faber::display_bivalens(args.verbose));"));
}

#[test]
fn compile_package_rejects_mounted_local_binding_collision_with_root_global() {
    let dir = test_temp_dir("cli-mount-root-global-collision");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ optio verbose longum "verbose" typus bivalens ubique
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "run"
@ optio verbose longum "local-verbose"
functio run() {}
"#,
    )
    .expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "local_option_global_collision")));
}

#[test]
fn compile_package_rejects_named_import_mount_targets() {
    let dir = test_temp_dir("cli-mount-named");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./jobs" privata set_config ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        "@ imperium \"run\"\nfunctio set_config() {}",
    )
    .expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        "@ imperium \"run\"\n@ optio verbose longum \"verbose\" ubique\nfunctio run() {}",
    )
    .expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "global_option_placement")));
}

#[test]
fn compile_package_rejects_mounted_command_path_collisions() {
    let dir = test_temp_dir("cli-mount-collision");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}

@ imperium "jobs/run"
functio root_run() {}
"#,
    )
    .expect("write entry");
    fs::write(dir.join("jobs.fab"), "@ imperium \"run\"\nfunctio run() {}").expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperium "one"
@ alias "same"
functio one() {}

@ imperium "two"
@ alias "same"
functio two() {}
"#,
    )
    .expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
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

#[test]
fn compile_package_does_not_expose_unmounted_imported_cli_modules() {
    let dir = test_temp_dir("cli-unmounted");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./jobs" privata * ut jobs

@ cli "tool"
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(dir.join("jobs.fab"), "@ imperium \"run\"\nfunctio run() {}").expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
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
        "importa ex \"./jobs\" privata * ut jobs\nincipit {}",
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        "importa ex \"./main\" privata * ut main\nfunctio run() {}",
    )
    .expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "package_import_cycle")));
}

#[test]
fn compile_package_supports_manifest_example() {
    let dir = test_temp_dir("manifest");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "incipit {}").expect("write package entry");
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

    let result = compile_package(&Config::default(), &dir.join("faber.toml"));
    assert!(result.success(), "expected package compile success");
}

#[test]
fn compile_package_discovers_faber_toml_from_directory() {
    let dir = test_temp_dir("manifest-dir");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "incipit { nota \"ok\" }").expect("write package entry");
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

    let result = compile_package(&Config::default(), &dir);
    assert!(
        result.success(),
        "expected package directory compile success"
    );
}

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
        "functio exsequi(textus via) → textus\nfunctio local() → textus { redde \"ok\" }\n",
    )
    .expect("write source");
    fs::write(
        dir.join("rust/shim.rs"),
        "pub fn exsequi(via: String) -> String { via }\n",
    )
    .expect("write shim");
    fs::write(
        dir.join("bindings/rust.toml"),
        r#"[functions."sqlite:sqlite.exsequi"]
symbol = "crate::shim::exsequi"

[shim]
path = "rust/shim.rs"
"#,
    )
    .expect("write bindings");

    let report = verify_library_bindings(&dir, "rust").expect("verify bindings");
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
        "functio exsequi(textus via) → textus\n",
    )
    .expect("write source");
    fs::write(dir.join("bindings/rust.toml"), "").expect("write bindings");

    let diagnostics = verify_library_bindings(&dir, "rust").expect_err("missing binding");
    assert!(diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "binding_required_missing")));
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
        "functio local() → textus { redde \"ok\" }\n",
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
    assert!(diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "binding_unknown_declaration")));
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
    assert_eq!(manifest.build.target, "rust");
    assert!(manifest.build.targets.is_empty());
    assert_eq!(manifest.build.kind, "bin");
    assert!(manifest.reader.locale.is_none());
    assert!(manifest.reader.pack.is_none());
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

    let result = compile_package(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "missing_binary_entry")));
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

    let result = compile_package(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "invalid_package_name")));
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

    let result = compile_package(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "invalid_library_provider")));
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

    let result = compile_package(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "missing_library_targets")));
}

#[test]
fn compile_package_uses_manifest_reader_locale_default_pack() {
    let dir = test_temp_dir("manifest-reader-locale");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "入口 { 输出 \"ok\" }").expect("write package entry");
    write_zh_reader_pack(&dir, "zh-Hans.toml");
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

    let result = compile_package(&Config::default(), &dir);

    assert!(
        result.success(),
        "expected manifest reader locale package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

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

    let result = compile_package(&Config::default(), &dir);

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

#[test]
fn installed_reader_locale_reference_examples_compile_from_installed_packs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale");

    for (locale, path, function, binding, greeting) in [
        (
            "zh-Hans",
            "zh-Hans",
            "fn 问候(名字: String) -> String",
            "let 问候语: String",
            "你好",
        ),
        (
            "zh-Hant",
            "zh-Hant",
            "fn 問候(名字: String) -> String",
            "let 問候語: String",
            "你好",
        ),
        (
            "ar",
            "ar",
            "fn تحية(اسم: String) -> String",
            "let رسالة: String",
            "مرحبا",
        ),
        (
            "hi",
            "hi",
            "fn अभिवादन(नाम: String) -> String",
            "let संदेश: String",
            "नमस्ते",
        ),
        (
            "vi",
            "vi",
            "fn chào(tên: String) -> String",
            "let lời_chào: String",
            "Xin chào",
        ),
    ] {
        let example = root.join(path);
        let result = compile_package(&Config::default(), &example);

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
}

#[test]
fn compile_package_uses_manifest_reader_pack_path() {
    let dir = test_temp_dir("manifest-reader-pack");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "入口 { 输出 \"ok\" }").expect("write package entry");
    write_zh_reader_pack(&dir, "custom.toml");
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
pack = "./reader/custom.toml"
"#,
    )
    .expect("write manifest");

    let result = compile_package(&Config::default(), &dir);

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn package_reader_locale_cli_selection_overrides_manifest_locale() {
    let dir = test_temp_dir("reader-cli-override");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "入口 { 输出 \"ok\" }").expect("write package entry");
    write_zh_reader_pack(&dir, "zh-Hans.toml");
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
        config_with_reader_locale(Target::Rust, &dir, Some("zh-Hans")).expect("reader config");
    let pack = pack.expect("reader pack");

    assert_eq!(pack.metadata.id, "zh-Hans");
    assert!(config.reader_pack.is_some());
}

#[test]
fn compile_package_reports_manifest_reader_locale_latin_fallback_warning() {
    let dir = test_temp_dir("manifest-reader-fallback");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "incipit { nota \"ok\" }").expect("write package entry");
    write_zh_reader_pack(&dir, "zh-Hans.toml");
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

    let result = compile_package(&Config::default(), &dir);

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == Some("READER001")));
}

#[test]
fn thai_reader_locale_example_compiles_from_manifest() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale/th-TH");

    let result = compile_package(&Config::default(), &example);

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected generated Rust output");
    };
    let rust = output.code;
    assert!(rust.contains("fn ทักทาย(name: String) -> String"));
    assert!(rust.contains("let greeting: String = format!(\"สวัสดี, {}!\", name);"));
    assert!(rust.contains("fn ผ่าน(score: i64) -> bool"));
    assert!(rust.contains("if score >= 80"));
    assert!(rust.contains("return true;"));
    assert!(rust.contains("return false;"));
    assert!(rust.contains("fn นับผ่าน(scores: Vec<i64>) -> i64"));
    assert!(rust.contains("for i1000005 in &(scores)"));
    assert!(rust.contains("continue;"));
    assert!(rust.contains("break;"));
    assert!(rust.contains("fn นับถอยหลัง(start: i64) -> i64"));
    assert!(rust.contains("while current > 0"));
    assert!(rust.contains("let score: i64 = 82;"));
    assert!(rust.contains("let scores: Vec<i64> = vec!["));
    assert!(rust.contains("faber::display_bivalens(ผ่าน(score))"));
    assert!(rust.contains("นับผ่าน(scores.clone())"));
    assert!(rust.contains("นับถอยหลัง(3)"));
}

#[test]
fn compile_package_rejects_unsupported_manifest_target() {
    let dir = test_temp_dir("manifest-target");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("main.fab"), "incipit {}").expect("write package entry");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "bad-target"

[paths]
entry = "main.fab"

[build]
target = "go"
"#,
    )
    .expect("write manifest");

    let result = compile_package(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| diagnostic_has_issue(
        diag,
        "package_build_target_unsupported"
    ) && diagnostic_has_arg(diag, "target", "go")));
}

#[test]
fn discover_package_accepts_manifest_package_mir_targets() {
    for target in ["rust", "scena", "fmir-text", "fmir", "fmir-bin"] {
        let dir = test_temp_dir(&format!("manifest-target-{}", target.replace('-', "_")));
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src");
        fs::write(src.join("main.fab"), "incipit {}").expect("write package entry");
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
importa ex "./jobs" privata * ut jobs

@ cli "tool"
@ imperia "jobs" ex jobs
incipit argumenta args {}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("jobs.fab"),
        r#"
@ imperia "again" ex jobs
@ imperium "run"
functio run() {}
"#,
    )
    .expect("write jobs");

    let result = compile_package(&Config::default(), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| diag
        .args
        .contains(&DiagnosticArg::new("issue", "nested_module_mount"))));
}

// ---------------------------------------------------------------------------
// Frontmatter package loading (Stage 4)
// ---------------------------------------------------------------------------

#[test]
fn load_package_peels_frontmatter_before_parse() {
    let dir = test_temp_dir("frontmatter-peel");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
sectio = "smoke"
group = "demo.entry"
+++

incipit { nota "peeled" }
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let files = load_package(&spec, &library_resolver_from_config(&config)).expect("load");
    let file = files
        .iter()
        .find(|file| file.path == entry)
        .expect("entry file");

    assert!(!file.source.contains("+++"));
    assert!(!file.source.contains("sectio"));
    assert!(file.raw_source.contains("+++"));
    assert_eq!(
        file.frontmatter.as_ref().and_then(|fm| fm.sectio()),
        Some("smoke")
    );
    assert_eq!(
        file.frontmatter.as_ref().and_then(|fm| fm.group()),
        Some("demo.entry")
    );
    assert_eq!(file.module_segments, vec!["demo", "entry"]);
}

#[test]
fn load_package_rejects_invalid_frontmatter_toml() {
    let dir = test_temp_dir("frontmatter-invalid");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
sectio = 
+++

incipit {}
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let result = load_package(&spec, &library_resolver_from_config(&config));
    assert!(result.is_err());
    let err = result.err().expect("diagnostics");
    assert!(err.iter().any(|diag| diag.code == Some("PARSE052")));
}

#[test]
fn load_package_rejects_frontmatter_manifest_build_conflict() {
    let dir = test_temp_dir("frontmatter-manifest-conflict");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "conflict-demo"

[paths]
entry = "main.fab"

[build]
target = "rust"
"#,
    )
    .expect("manifest");
    fs::write(
        dir.join("src/main.fab"),
        r#"+++
[build]
target = "ts"
+++

incipit {}
"#,
    )
    .expect("entry");

    let config = Config::default();
    let spec = discover_package(&dir).expect("package");
    let result = load_package(&spec, &library_resolver_from_config(&config));
    assert!(result.is_err());
    let err = result.err().expect("diagnostics");
    assert!(err.iter().any(|diag| {
        diagnostic_has_issue(diag, "frontmatter_manifest_override")
            && diagnostic_has_arg(diag, "frontmatter", "[build].target")
            && diagnostic_has_arg(diag, "frontmatter_value", "ts")
            && diagnostic_has_arg(diag, "manifest", "target")
            && diagnostic_has_arg(diag, "manifest_value", "rust")
    }));
}

#[test]
fn compile_package_honors_group_frontmatter_for_module_tree() {
    let dir = test_temp_dir("frontmatter-group");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./lib" privata lib

incipit {
    nota lib.answer()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("lib.fab"),
        r#"+++
group = "custom.lib"
+++

functio answer() → numerus {
    redde 42
}
"#,
    )
    .expect("write lib");

    let result = compile_package(&Config::default(), &dir.join("main.fab"));
    assert!(
        result.success(),
        "expected group frontmatter package compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output.code.contains("pub mod custom"));
    assert!(output.code.contains("pub mod lib"));
}

#[test]
fn compile_package_exports_imported_module_functions_to_entry() {
    let dir = test_temp_dir("package-import-function");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./auxilium" privata aux

incipit {
    nota aux.saluta("Marcus")
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
functio saluta(textus nomen) → textus {
    redde "Salve, §!"(nomen)
}
"#,
    )
    .expect("write module");

    let result = compile_package(&Config::default(), &dir.join("main.fab"));
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

    assert!(output
        .code
        .contains("crate::auxilium::saluta(\"Marcus\".to_string())"));
    assert!(!output.code.contains("use crate::auxilium::aux"));
    assert!(output.code.contains("pub(crate) fn saluta"));
    assert!(!output.code.contains("#![allow(non_snake_case)]"));
    let main_pos = output.code.find("fn main()").expect("entry main");
    let module_pos = output.code.find("pub mod auxilium").expect("module");
    assert!(
        main_pos < module_pos,
        "package modules must be emitted after the entry body when no crate-level inner attributes remain"
    );
}

#[test]
fn compile_package_calls_top_level_function_through_file_namespace_alias() {
    let dir = test_temp_dir("package-file-namespace-function");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./auxilium" privata Aux

incipit {
    nota Aux.adde(2, 3)
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
functio adde(numerus left, numerus right) → numerus {
    redde left + right
}
"#,
    )
    .expect("write module");

    let result = compile_package(&Config::default(), &dir.join("main.fab"));
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
importa ex "./auxilium" privata Aux

incipit {
    fixum textus label ← Aux.label("Marcus")
    nota label
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
functio label(textus nomen) → textus {
    redde "Salve, §!"(nomen)
}
"#,
    )
    .expect("write module");

    let diagnostics = check_package(&Config::default(), &entry);
    assert!(
        !diagnostics.iter().any(|diag| diag.is_error()),
        "expected typed file namespace function call, got {:?}",
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
importa ex "./auxilium" privata Aux

incipit {
    nota Aux.label(42)
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
functio label(textus nomen) → textus {
    redde nomen
}
"#,
    )
    .expect("write module");

    let diagnostics = check_package(&Config::default(), &entry);
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
fn check_package_types_norma_solum_path_namespace_call() {
    let dir = test_temp_dir("package-file-namespace-solum-path-type");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:solum/path" privata path

incipit {
    fixum textus name ← path.nomen("/usr/bin/faber")
    nota name
}
"#,
    )
    .expect("write entry");

    let diagnostics = check_package(&Config::default(), &entry);
    assert!(
        !diagnostics.iter().any(|diag| diag.is_error()),
        "expected typed solum/path namespace call, got {:?}",
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
        &library_home,
        "json/solve",
        r#"
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
importa ex "norma:json/solve" privata solve

incipit {
    fixum valor doc ← solve.solve("{}")
    nota doc
}
"#,
    )
    .expect("write entry");

    let diagnostics = check_package(&config_with_library_home(&library_home), &entry);
    assert!(
        !diagnostics.iter().any(|diag| diag.is_error()),
        "expected typed top-level json/solve namespace call, got {:?}",
        diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn build_package_routes_duplicate_stem_file_namespace_imports_by_path() {
    let dir = test_temp_dir("package-file-namespace-duplicate-stems");
    fs::create_dir_all(dir.join("a")).expect("create a dir");
    fs::create_dir_all(dir.join("b")).expect("create b dir");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "./a/util" privata A
importa ex "./b/util" privata B

incipit {
    nota A.label()
    nota B.label()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("a").join("util.fab"),
        r#"
functio label() → textus {
    redde "A"
}
"#,
    )
    .expect("write a util");
    fs::write(
        dir.join("b").join("util.fab"),
        r#"
functio label() → textus {
    redde "B"
}
"#,
    )
    .expect("write b util");

    let stdout = compile_emit_build_run(&entry);

    assert_eq!(stdout.lines().collect::<Vec<_>>(), vec!["A", "B"]);
}

#[test]
fn build_package_supplies_namespace_metadata_to_non_entry_modules() {
    let dir = test_temp_dir("package-file-namespace-non-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "./service" privata Service

incipit {
    Service.run()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("service.fab"),
        r#"
importa ex "./util" privata U

functio run() → vacuum {
    nota U.adde(2, 3)
}
"#,
    )
    .expect("write service");
    fs::write(
        dir.join("util.fab"),
        r#"
functio adde(numerus left, numerus right) → numerus {
    redde left + right
}
"#,
    )
    .expect("write util");

    let stdout = compile_emit_build_run(&entry);

    assert_eq!(stdout.trim(), "5");
}

#[test]
fn build_package_keeps_duplicate_qualified_type_names_namespace_scoped() {
    let dir = test_temp_dir("package-file-namespace-duplicate-types");
    fs::create_dir_all(dir.join("a")).expect("create a dir");
    fs::create_dir_all(dir.join("b")).expect("create b dir");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "./a/types" privata A
importa ex "./b/types" privata B

incipit {
    fixum A.Thing a ← A.Thing { label = "A" }
    fixum B.Thing b ← B.Thing { label = "B" }
    nota a.label
    nota b.label
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("a").join("types.fab"),
        r#"
genus Thing {
    textus label
}
"#,
    )
    .expect("write a types");
    fs::write(
        dir.join("b").join("types.fab"),
        r#"
genus Thing {
    textus label
}
"#,
    )
    .expect("write b types");

    let stdout = compile_emit_build_run(&entry);

    assert_eq!(stdout.lines().collect::<Vec<_>>(), vec!["A", "B"]);
}

#[test]
fn compile_package_calls_file_function_through_file_namespace_alias() {
    let dir = test_temp_dir("package-file-namespace-function");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./auxilium" privata Aux

incipit {
    nota Aux.dupla(4)
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
functio dupla(numerus n) → numerus {
    redde n * 2
}
"#,
    )
    .expect("write module");

    let result = compile_package(&Config::default(), &dir.join("main.fab"));
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
importa ex "./auxilium" privata Aux

incipit {
    nota Aux.secretum()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
@ privata
functio secretum() → textus {
    redde "hidden"
}

functio publicum() → textus {
    redde secretum()
}
"#,
    )
    .expect("write module");

    let result = compile_package(&Config::default(), &dir.join("main.fab"));
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "namespace_missing_export")
            && diagnostic_has_arg(diag, "member", "secretum")
    }));
}

#[test]
fn compile_package_exports_publica_function_through_file_namespace_alias() {
    let dir = test_temp_dir("package-publica-file-namespace-function");
    fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./auxilium" privata Aux

incipit {
    nota Aux.publicum()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("auxilium.fab"),
        r#"
@ publica
functio publicum() → textus {
    redde "visible"
}
"#,
    )
    .expect("write module");

    let result = compile_package(&Config::default(), &dir.join("main.fab"));
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
importa ex "./parser" privata P

incipit {
    nota P.scan("{}")
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("parser.fab"),
        r#"
@ privata
functio scan(textus input) → textus {
    redde input
}

functio solve(textus input) → textus {
    redde input
}
"#,
    )
    .expect("write module");

    let result = compile_package(&Config::default(), &dir.join("main.fab"));
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "namespace_missing_export")
            && diagnostic_has_arg(diag, "member", "scan")
    }));
}

#[test]
fn compile_package_rejects_private_library_type_through_file_namespace_alias() {
    let library_home = test_temp_dir("private-library-type-library_home");
    write_temp_library_fixture(
        &library_home,
        "types",
        r#"
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
importa ex "norma:types" privata types

incipit {
    fixum types.Secretum secretum ← types.Secretum {}
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&config_with_library_home(&library_home), &entry);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| { diagnostic_has_issue(diag, "qualified_type_not_exported") }));
}

#[test]
fn compile_package_rejects_private_library_function_through_file_namespace_alias() {
    let library_home = test_temp_dir("private-library-function-library_home");
    write_temp_library_fixture(
        &library_home,
        "helpers",
        r#"
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
importa ex "norma:helpers" privata helpers

incipit {
    nota helpers.secretum()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&config_with_library_home(&library_home), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "namespace_missing_export")
            && diagnostic_has_arg(diag, "member", "secretum")
    }));
}

#[test]
fn compile_package_applies_entry_frontmatter_test_selection_defaults() {
    let dir = test_temp_dir("frontmatter-test-defaults");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
sectio = "outer suite/inner suite"

[probanda]
tags = ["focus"]
+++

proba "name match" {
    adfirma verum
}

probandum "outer suite" {
    probandum "inner suite" {
        proba "wrong tag" tag "smoke" {
            adfirma verum
        }

        proba "combined match" tag "focus" {
            adfirma verum
        }
    }
}

incipit {}
"#,
    )
    .expect("write entry");

    let result = compile_package_with_test_selection(&Config::default(), &entry, None);
    assert!(
        result.success(),
        "expected frontmatter test-default compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(output
        .code
        .contains("#[ignore = \"faber: not selected by suite outer suite/inner suite\"]"));
    assert!(output
        .code
        .contains("#[ignore = \"faber: not selected by tag focus\"]"));
}

#[test]
fn compile_package_cli_test_selection_overrides_entry_frontmatter_defaults() {
    let dir = test_temp_dir("frontmatter-test-cli-override");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"+++
sectio = "outer suite/inner suite"

[probanda]
tags = ["focus"]
+++

proba "tag match" tag "smoke" {
    adfirma verum
}

proba "other tag" tag "focus" {
    adfirma verum
}

incipit {}
"#,
    )
    .expect("write entry");

    let selection = TestSelection {
        tag: Some("smoke".to_owned()),
        ..TestSelection::default()
    };
    let result = compile_package_with_test_selection(&Config::default(), &entry, Some(&selection));
    assert!(
        result.success(),
        "expected CLI override compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };

    assert!(!output.code.contains("not selected by suite"));
    assert!(output
        .code
        .contains("#[ignore = \"faber: not selected by tag smoke\"]"));
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
    assert!(layout
        .debug_binary
        .to_string_lossy()
        .ends_with("manifest-pkg"));
}

#[test]
fn discover_build_layout_supports_directory_with_manifest() {
    let dir = test_temp_dir("layout-dir-manifest");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(dir.join("src/main.fab"), "incipit {}").expect("entry");
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

#[test]
fn direct_entry_file_under_manifest_uses_package_root_and_manifest_name() {
    let dir = test_temp_dir("layout-entry-parent-manifest");
    fs::create_dir_all(dir.join("src")).expect("src");
    let entry = dir.join("src/main.fab");
    fs::write(&entry, "incipit { nota \"manifest root\" }").expect("entry");
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
    assert!(!layout
        .generated_rust_entry
        .to_string_lossy()
        .contains("/src/target/faber/"));
}

#[test]
fn direct_non_manifest_entry_under_manifest_keeps_explicit_entry() {
    let dir = test_temp_dir("layout-explicit-entry-parent-manifest");
    fs::create_dir_all(dir.join("src")).expect("src");
    let manifest_entry = dir.join("src/main.fab");
    let explicit_entry = dir.join("src/other.fab");
    fs::write(&manifest_entry, "incipit { nota \"manifest entry\" }").expect("manifest entry");
    fs::write(&explicit_entry, "incipit { nota \"explicit entry\" }").expect("explicit entry");
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

    let output = compile_package(&Config::default(), &explicit_entry)
        .output
        .expect("compile explicit entry");
    let rust = match output {
        radix::Output::Rust(output) => output.code,
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

#[test]
fn discover_build_layout_supports_entry_file_input_and_falls_back_to_dir_name() {
    let dir = test_temp_dir("layout-entry-no-manifest");
    let entry = dir.join("main.fab");
    fs::write(&entry, "incipit { nota \"x\" }").expect("entry");

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

// ---------------------------------------------------------------------------
// Phase 2: Generated crate writer tests (no Cargo invocation)
// ---------------------------------------------------------------------------

#[test]
fn emit_generated_crate_writes_cargo_toml_and_main_rs_under_target_faber() {
    let pkg = test_temp_dir("emit-writer");
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("src/main.fab"),
        r#"incipit { nota "writer test" }"#,
    )
    .expect("entry");
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
    let compile_result = compile_package(&Config::default(), &pkg);
    assert!(compile_result.success(), "compile should succeed");
    let code = match &compile_result.output {
        Some(radix::Output::Rust(r)) => r.code.clone(),
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
    assert!(cargo_toml.contains("package = \"faber-runtime\"") && cargo_toml.contains("path = "));
    assert!(!cargo_toml.contains("norma = { path = "));
    assert!(!cargo_toml.contains("tokio = { version = "));

    let main_rs = fs::read_to_string(&layout.generated_rust_entry).expect("read main");
    assert!(main_rs.contains("Generated by faber build"));
    assert!(main_rs.contains("writer test")); // from the source string

    // No nested target created by the writer
    assert!(!layout.generated_crate_root.join("target").exists());
}

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
        r#"incipit { fixum textus body ← ad 'runtime:echo' ("ok") ↦ textus nota body }"#,
    )
    .expect("entry");

    let plan = package_rust_runtime_plan(&Config::default(), &pkg).expect("runtime plan");

    assert!(plan.non_runtime_routes.is_empty());
    assert!(plan.host.is_none());
    assert!(!plan.needs_tokio);
}

#[test]
fn package_runtime_plan_requires_host_for_non_runtime_routes() {
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
        r#"incipit { fixum textus body ← ad 'solum:lege' ("data.txt") ↦ textus nota body }"#,
    )
    .expect("entry");

    let plan = package_rust_runtime_plan(&Config::default(), &pkg).expect("runtime plan");

    assert_eq!(
        plan.non_runtime_routes.iter().cloned().collect::<Vec<_>>(),
        vec!["solum:lege".to_owned()]
    );
    assert_eq!(
        plan.selected_providers.iter().cloned().collect::<Vec<_>>(),
        vec!["solum".to_owned()]
    );
    assert!(plan.host.is_none());
    let diagnostic = package_host_selection_diagnostic(&plan, &pkg.join("faber.toml"))
        .expect("missing host selection diagnostic");
    assert!(diagnostic_has_issue(
        &diagnostic,
        "package_host_selection_required"
    ));
}

#[test]
fn package_runtime_plan_rejects_unknown_native_provider() {
    let pkg = test_temp_dir("runtime-plan-unknown-provider");
    fs::create_dir_all(pkg.join("src")).expect("src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "runtime-plan-unknown-provider"

[paths]
source = "src"
entry = "main.fab"

[target.rust]
host = "native"

[dispatch]
providers = ["notaprovider"]
"#,
    )
    .expect("manifest");
    fs::write(
        pkg.join("src/main.fab"),
        r#"incipit { fixum textus body ← ad 'runtime:echo' ("ok") ↦ textus nota body }"#,
    )
    .expect("entry");

    let plan = package_rust_runtime_plan(&Config::default(), &pkg).expect("runtime plan");
    assert_eq!(plan.host, Some(ManifestRustHost::Native));
    assert!(plan.selected_providers.contains("notaprovider"));
    assert!(
        plan.provider_error.is_some(),
        "expected missing provider error"
    );
    let diagnostic = package_host_selection_diagnostic(&plan, &pkg.join("faber.toml"))
        .expect("provider selection diagnostic");
    assert!(diagnostic_has_issue(
        &diagnostic,
        "host_provider_selection_invalid"
    ));
}

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
        r#"incipiet { fixum textus body ← ad 'runtime:echo' ("ok") ↦ textus nota body }"#,
    )
    .expect("entry");

    let layout = discover_build_layout(&pkg).expect("layout");
    let package = analyze_package(&Config::default(), &pkg).expect("analyze");
    let artifact = super::artifact_plan::plan_package(&package, Target::Rust);
    assert!(
        artifact.has_runtime_dependency("rust:runtime:faber"),
        "artifact plan must list faber runtime"
    );
    assert!(
        artifact.has_runtime_dependency("rust:runtime:tokio"),
        "async entry must plan tokio runtime node"
    );

    let plan = package_rust_runtime_plan(&Config::default(), &pkg).expect("runtime plan");
    assert!(plan.needs_faber);
    assert!(plan.needs_tokio);

    let compile_result = compile_package(&Config::default(), &pkg);
    assert!(compile_result.success(), "compile should succeed");
    let code = match &compile_result.output {
        Some(radix::Output::Rust(r)) => r.code.clone(),
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
    assert!(cargo_toml.contains("faber = { package = \"faber-runtime\""));
    assert!(cargo_toml.contains("tokio = { version = "));
}

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
        r#"incipiet { fixum textus body ← ad 'runtime:echo' ("ok") ↦ textus nota body }"#,
    )
    .expect("entry");

    let layout = discover_build_layout(&pkg).expect("layout");
    let compile_result = compile_package(&Config::default(), &pkg);
    assert!(compile_result.success(), "compile should succeed");
    let code = match &compile_result.output {
        Some(radix::Output::Rust(r)) => r.code.clone(),
        _ => panic!("expected rust output"),
    };
    let plan = package_rust_runtime_plan(&Config::default(), &pkg).expect("runtime plan");
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

#[test]
fn emit_generated_crate_works_without_manifest_using_fallback_name() {
    let pkg = test_temp_dir("emit-no-manifest");
    let entry = pkg.join("main.fab");
    fs::write(&entry, "incipit {}").expect("entry");

    let layout = discover_build_layout(&entry).expect("layout");
    // Directly test the emit path with dummy code (no real compile needed for writer coverage)
    let dummy = "fn main(){}";
    let _ = emit_generated_crate(&layout, dummy, None).expect("emit fallback");

    let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo");
    assert!(cargo.contains(&format!("name = \"{}\"", layout.binary_name())));
}

#[test]
fn generated_package_ad_avoids_private_host_bridge_dependency() {
    let pkg = test_temp_dir("package-ad-runtime-route");
    let data = pkg.join("data.txt");
    fs::write(&data, "salve host").expect("write data fixture");
    let path_lit = format!("{:?}", data.to_string_lossy());
    let entry = pkg.join("main.fab");
    fs::write(
        &entry,
        format!(
            r#"
incipit {{
    fixum textus body ← ad 'solum:lege' ({path_lit}) ↦ textus
    nota body
}}
"#
        ),
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
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
    assert!(!output.code.contains("__faber_attach_sermo"));

    let layout = discover_build_layout(&entry).expect("layout");
    emit_generated_crate(&layout, &output.code, None).expect("emit generated crate");
    let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo");
    assert!(!cargo.contains("faber-host-macos-arm64"));
    assert!(!cargo.contains("__faber_host_macos_arm64"));
    assert!(!cargo.contains("../radix/hosts/macos-arm64"));

    let binary = invoke_cargo_build(&layout, false).expect("cargo build");
    let run = Command::new(binary).output().expect("run generated binary");
    assert!(run.status.success(), "generated binary failed: {:?}", run);
    assert_eq!(
        String::from_utf8(run.stdout).expect("stdout utf8"),
        "salve host\n"
    );
}

#[test]
fn generated_package_native_host_selects_public_dependency_and_runs() {
    let pkg = test_temp_dir("package-native-host-route");
    fs::create_dir_all(pkg.join("src")).expect("create src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "native-host-route"

[paths]
source = "src"
entry = "main.fab"

[target.rust]
host = "native"
"#,
    )
    .expect("write manifest");
    let data = pkg.join("data.txt");
    fs::write(&data, "salve native").expect("write data fixture");
    let path_lit = format!("{:?}", data.to_string_lossy());
    fs::write(
        pkg.join("src/main.fab"),
        format!(
            r#"
incipit {{
    fixum textus body ← ad 'solum:lege' ({path_lit}) ↦ textus
    nota body
}}
"#
        ),
    )
    .expect("write entry");

    let layout = discover_build_layout(&pkg).expect("layout");
    let compile_result = compile_package(&Config::default(), &pkg);
    assert!(compile_result.success(), "compile should succeed");
    let code = match &compile_result.output {
        Some(radix::Output::Rust(r)) => r.code.clone(),
        _ => panic!("expected rust output"),
    };
    let plan = package_rust_runtime_plan(&Config::default(), &pkg).expect("runtime plan");
    assert_eq!(plan.host, Some(ManifestRustHost::Native));
    assert_eq!(
        plan.selected_providers.iter().cloned().collect::<Vec<_>>(),
        vec!["solum".to_owned()]
    );
    assert!(plan.provider_error.is_none(), "{:?}", plan.provider_error);
    assert!(
        plan.provider_manifests
            .iter()
            .any(|manifest| manifest.provider == "solum"),
        "expected solum provider manifest"
    );

    emit_generated_crate_with_runtime_plan(
        &layout,
        &code,
        Some(&read_manifest(&layout.manifest_path).unwrap()),
        &plan,
    )
    .expect("emit generated crate");
    let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo");
    assert!(cargo.contains("host-kernel"));
    assert!(cargo.contains("host_kernel"));
    assert!(cargo.contains("host-native"));
    assert!(cargo.contains("host_native"));
    assert!(cargo.contains("solum"));
    assert!(!cargo.contains("faber-host-native"));
    assert!(!cargo.contains("faber_host_native"));
    assert!(!cargo.contains("faber-host-macos-arm64"));
    assert!(!cargo.contains("../radix/hosts/macos-arm64"));
    let generated_main = fs::read_to_string(&layout.generated_rust_entry).expect("main");
    assert!(generated_main.contains("mod host_register;"));
    assert!(generated_main.contains("host_register::install_or_exit();"));
    let host_register = fs::read_to_string(
        layout
            .generated_crate_root
            .join("src")
            .join("host_register.rs"),
    )
    .expect("host_register");
    assert!(host_register.contains("solum::register(&mut kernel)"));
    assert!(host_register.contains("host_native::NativeHost::new(kernel)"));
    assert!(host_register.contains("faber::install_host_dispatch"));
    let host_manifest = fs::read_to_string(layout.generated_crate_root.join("host-manifest.json"))
        .expect("host-manifest.json");
    assert!(host_manifest.contains("solum:lege"));
    assert!(host_manifest.contains("\"provider\": \"solum\""));

    let binary = invoke_cargo_build(&layout, false).expect("cargo build");
    let run = Command::new(binary).output().expect("run generated binary");
    assert!(
        run.status.success(),
        "generated binary failed: status={:?} stdout={:?} stderr={:?}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8(run.stdout).expect("stdout utf8"),
        "salve native\n"
    );
}

#[test]
fn generated_package_native_host_async_solum_runs() {
    // P6 package E2E: async entry + public native host + solum provider.
    // Cancel/saturation/shutdown remain proven in host-native unit tests;
    // package path proves install + async dispatch through HostDispatch.
    let pkg = test_temp_dir("package-native-host-async-solum");
    fs::create_dir_all(pkg.join("src")).expect("create src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "native-host-async-solum"

[paths]
source = "src"
entry = "main.fab"

[target.rust]
host = "native"
"#,
    )
    .expect("write manifest");
    let data = pkg.join("data.txt");
    fs::write(&data, "salve async").expect("write data fixture");
    let path_lit = format!("{:?}", data.to_string_lossy());
    fs::write(
        pkg.join("src/main.fab"),
        format!(
            r#"
incipiet {{
    fixum textus body ← ad 'solum:lege' ({path_lit}) ↦ textus
    nota body
}}
"#
        ),
    )
    .expect("write entry");

    let layout = discover_build_layout(&pkg).expect("layout");
    let compile_result = compile_package(&Config::default(), &pkg);
    assert!(
        compile_result.success(),
        "compile should succeed: {:?}",
        compile_result.diagnostics
    );
    let code = match &compile_result.output {
        Some(radix::Output::Rust(r)) => r.code.clone(),
        _ => panic!("expected rust output"),
    };
    let plan = package_rust_runtime_plan(&Config::default(), &pkg).expect("runtime plan");
    assert_eq!(plan.host, Some(ManifestRustHost::Native));
    assert!(plan.needs_tokio);
    assert!(plan.selected_providers.contains("solum"));
    assert!(plan.provider_error.is_none(), "{:?}", plan.provider_error);

    emit_generated_crate_with_runtime_plan(
        &layout,
        &code,
        Some(&read_manifest(&layout.manifest_path).unwrap()),
        &plan,
    )
    .expect("emit generated crate");
    let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo");
    assert!(cargo.contains("host_native"));
    assert!(cargo.contains("solum"));
    assert!(cargo.contains("tokio = { version = "));
    assert!(!cargo.contains("../radix/hosts/macos-arm64"));
    let generated_main = fs::read_to_string(&layout.generated_rust_entry).expect("main");
    assert!(generated_main.contains("host_register::install_or_exit();"));
    assert!(
        generated_main.contains("__faber_block_on") || generated_main.contains("tokio::"),
        "expected async runtime wiring in generated main"
    );

    let binary = invoke_cargo_build(&layout, false).expect("cargo build");
    let run = Command::new(binary).output().expect("run generated binary");
    assert!(
        run.status.success(),
        "generated async binary failed: status={:?} stdout={:?} stderr={:?}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8(run.stdout).expect("stdout utf8"),
        "salve async\n"
    );
}

#[test]
fn generated_package_native_host_rejects_unknown_route_provider() {
    let pkg = test_temp_dir("package-native-host-unknown-route");
    fs::create_dir_all(pkg.join("src")).expect("create src");
    fs::write(
        pkg.join("faber.toml"),
        r#"
[package]
name = "native-host-unknown-route"

[paths]
source = "src"
entry = "main.fab"

[target.rust]
host = "native"
"#,
    )
    .expect("write manifest");
    fs::write(
        pkg.join("src/main.fab"),
        r#"incipit { fixum textus body ← ad 'ignotum:route' ("x") ↦ textus nota body }"#,
    )
    .expect("write entry");

    let plan = package_rust_runtime_plan(&Config::default(), &pkg).expect("runtime plan");
    assert_eq!(plan.host, Some(ManifestRustHost::Native));
    assert!(plan.selected_providers.contains("ignotum"));
    assert!(plan.provider_error.is_some(), "expected missing provider");
    let diagnostic = package_host_selection_diagnostic(&plan, &pkg.join("faber.toml"))
        .expect("provider selection diagnostic");
    assert!(diagnostic_has_issue(
        &diagnostic,
        "host_provider_selection_invalid"
    ));
}

#[test]
fn generated_package_norma_json_facade_uses_formal_conversio() {
    let pkg = test_temp_dir("package-norma-json-formal-facade");
    let entry = pkg.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:json" privata json

incipit {
    fac {
        fixum json duplicate ← json.solve("{\"id\": 1, \"id\": 2}")
        nota duplicate
    }
    cape err {
        nota "duplicate rejected"
    }

    fac {
        fixum json unicode ← json.solve("{\"music\": \"\\uD834\\uDD1E\"}")
        nota json.pange(unicode)
    }
    cape err {
        nota err
    }
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "compile should succeed: {:?}",
        result.diagnostics
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("faber::Json::parse"));
    assert!(output.code.contains(".to_wire()"));
    assert!(!output
        .code
        .contains("non-BMP \\\\u escapes not yet supported"));
    assert!(!output.code.contains("out.pone"));
}

// ---------------------------------------------------------------------------
// Transitive library imports (norma: closure + interface provenance)
// ---------------------------------------------------------------------------

fn repo_norma_source_file(relative_path: &str) -> PathBuf {
    dev_norma_library_home()
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

fn seed_temp_library_chorda(library_home: &Path) {
    let source = repo_norma_source_file("chorda");
    let dest = library_home.join("norma/src/chorda.fab");
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).expect("create norma dir");
    }
    fs::copy(&source, &dest).expect("copy chorda.fab into temp library");
}

fn config_with_library_home(library_home: &Path) -> Config {
    Config::default().with_stdlib(library_home.to_path_buf())
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
functio label() → textus {
    redde "installed"
}
"#,
        )
        .expect("write installed module");
    }
}

#[test]
fn library_resolver_reports_installed_manifest_missing_source_root() {
    let library_home = test_temp_dir("installed-missing-source-root-home");
    write_installed_library_manifest(&library_home, "altlib", "interfaces", None);

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
importa ex "altlib:math/add" privata add

incipit {
    nota add.label()
}
"#,
    )
    .expect("write app entry");

    let result = compile_package(&config_with_library_home(&library_home), &dir);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "missing_installed_library_source_root")
            && diagnostic_has_arg(diag, "provider", "altlib")
    }));
}

fn transitive_test_diagnostic_facts(
    result: &radix::CompileResult,
) -> Vec<(Option<&str>, Option<&str>)> {
    result
        .diagnostics
        .iter()
        .map(|diag| (diag.code, diag.issue()))
        .collect()
}

#[test]
fn transitive_nested_lista_lista_textus_typechecks() {
    let library_home = test_temp_dir("transitive-nested-lista-library_home");
    write_temp_library_fixture(
        &library_home,
        "nested_lista_probe",
        r#"
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
importa ex "norma:nested_lista_probe" privata nested_lista_probe

incipit {
    fixum lista<lista<textus>> grid ← nested_lista_probe.vacua_tabula()
    nota grid.longitudo()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&config_with_library_home(&library_home), &entry);
    assert!(
        result.success(),
        "expected nested lista<lista<textus>> typecheck success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );
}

#[test]
fn transitive_import_chorda_via_fixture_library_compiles() {
    let library_home = test_temp_dir("transitive-chorda-library_home");
    seed_temp_library_chorda(&library_home);
    write_temp_library_fixture(
        &library_home,
        "transitive_fixture",
        r#"
importa ex "norma:chorda" privata chorda

functio split_fields(textus row) → lista<textus> {
    redde chorda.discissa(row, ",", row.longitudo() + 1)
}
"#,
    );

    let dir = test_temp_dir("transitive-chorda-entry");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:transitive_fixture" privata transitive_fixture

incipit {
    fixum lista<textus> fields ← transitive_fixture.split_fields("alpha,beta")
    nota fields.longitudo()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&config_with_library_home(&library_home), &entry);
    assert!(
        result.success(),
        "expected transitive chorda fixture compile success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );

    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("pub mod chorda"));
    assert!(output.code.contains("crate::chorda::discissa"));
    assert!(output.code.contains("pub mod transitive_fixture"));
    assert!(!output.code.contains("unresolved_def"));
}

#[test]
fn transitive_library_import_cycle_is_rejected() {
    let library_home = test_temp_dir("transitive-cycle-library_home");
    write_temp_library_fixture(
        &library_home,
        "cycle_a",
        r#"
importa ex "norma:cycle_b" privata cycle_b

functio label() → textus {
    redde "a"
}
"#,
    );
    write_temp_library_fixture(
        &library_home,
        "cycle_b",
        r#"
importa ex "norma:cycle_a" privata cycle_a

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
importa ex "norma:cycle_a" privata cycle_a

incipit {
    nota cycle_a.label()
}
"#,
    )
    .expect("write entry");

    let config = config_with_library_home(&library_home);
    let spec = discover_package(&entry).expect("package");
    let resolver = library_resolver_from_config(&config);
    let files = load_package(&spec, &resolver);
    assert!(files.is_err(), "cycle should fail package load");
    let err = files.err().expect("diagnostics");
    assert!(err.iter().any(|diag| {
        diagnostic_has_issue(diag, "library_import_cycle")
            && diagnostic_has_arg(diag, "cycle", "cycle_a -> cycle_b -> cycle_a")
    }));

    let result = compile_package(&config, &entry);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diagnostic_has_issue(diag, "library_import_cycle")));
}

#[test]
fn transitive_library_conflicting_aliases_are_rejected() {
    let dir = test_temp_dir("transitive-alias-conflict");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:csv" privata csv
importa ex "norma:chorda" privata chorda ut ropes

incipit {
    nota csv.solve("a,b").longitudo()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "library_conflicting_aliases")
            && diagnostic_has_arg(diag, "module", "chorda")
            && diagnostic_has_arg(diag, "alias", "ropes")
    }));
}

#[test]
fn transitive_library_diamond_dedupes_shared_dependency() {
    let library_home = test_temp_dir("transitive-diamond-library_home");
    write_temp_library_fixture(
        &library_home,
        "shared_d",
        r#"
functio tag() → textus {
    redde "d"
}
"#,
    );
    write_temp_library_fixture(
        &library_home,
        "dep_b",
        r#"
importa ex "norma:shared_d" privata shared_d

functio via_b() → textus {
    redde shared_d.tag()
}
"#,
    );
    write_temp_library_fixture(
        &library_home,
        "dep_c",
        r#"
importa ex "norma:shared_d" privata shared_d

functio via_c() → textus {
    redde shared_d.tag()
}
"#,
    );
    write_temp_library_fixture(
        &library_home,
        "leaf_a",
        r#"
importa ex "norma:dep_b" privata dep_b
importa ex "norma:dep_c" privata dep_c

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
importa ex "norma:leaf_a" privata leaf_a

incipit {
    nota leaf_a.both()
}
"#,
    )
    .expect("write entry");

    let config = config_with_library_home(&library_home);
    let spec = discover_package(&entry).expect("package");
    let resolver = library_resolver_from_config(&config);
    let files = load_package(&spec, &resolver).expect("load package");
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

    let package = analyze_package(&config, &entry).expect("analyze package");
    let analysis = &package.entry_unit().expect("entry unit").analysis;
    let leaf_a = library_binding_by_module(analysis, &["leaf_a"]);
    assert_eq!(leaf_a.identity.module_path, vec!["leaf_a"]);

    let result = compile_package(&config, &entry);
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
        &library_home,
        "child",
        r#"
functio label() → textus {
    redde "child"
}
"#,
    );
    write_temp_library_fixture(
        &library_home,
        "parent",
        r#"
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
importa ex "norma:parent" privata parent

incipit {
    nota parent.label()
}
"#,
    )
    .expect("write entry");

    let config = config_with_library_home(&library_home);
    let spec = discover_package(&entry).expect("package");
    let resolver = library_resolver_from_config(&config);
    let files = load_package(&spec, &resolver).expect("load package");
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
    assert_eq!(child.visibility, radix::syntax::Visibility::Publica);
    assert_eq!(parent.visibility, radix::syntax::Visibility::Privata);
}

#[test]
fn publica_library_import_surfaces_as_nested_namespace_binding() {
    let library_home = test_temp_dir("publica-reexport-library_home");
    write_temp_library_fixture(
        &library_home,
        "child",
        r#"
functio label() → textus {
    redde "child"
}
"#,
    );
    write_temp_library_fixture(
        &library_home,
        "parent",
        r#"
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
importa ex "norma:parent" privata parent

incipit {
    nota parent.child.label()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&config_with_library_home(&library_home), &entry);
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
        &library_home,
        "child",
        r#"
functio label() → textus {
    redde "child"
}
"#,
    );
    write_temp_library_fixture(
        &library_home,
        "parent",
        r#"
importa ex "norma:child" privata child

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
importa ex "norma:parent" privata parent

incipit {
    nota parent.child.label()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&config_with_library_home(&library_home), &entry);
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diag| {
        diagnostic_has_issue(diag, "namespace_missing_export")
            && diagnostic_has_arg(diag, "member", "child.label")
    }));
}

#[test]
fn transitive_csv_chorda_closure_compiles_with_mixed_provenance() {
    let dir = test_temp_dir("transitive-csv-chorda");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:csv" privata csv

incipit {
    fixum lista<lista<textus>> grid ← csv.solve("name,count\nalpha,1")
    fixum textus wire ← csv.pange(grid)
    nota wire
}
"#,
    )
    .expect("write entry");

    let config = Config::default();
    let spec = discover_package(&entry).expect("package");
    let resolver = library_resolver_from_config(&config);
    let files = load_package(&spec, &resolver).expect("load package");
    let file = files
        .iter()
        .find(|file| file.path == entry)
        .expect("entry file");
    assert_eq!(file.expanded_library_imports.len(), 2);

    let package = analyze_package(&config, &entry).expect("analyze package");
    let analysis = &package.entry_unit().expect("entry unit").analysis;

    let csv_binding = library_binding_by_module(analysis, &["csv"]);
    assert_eq!(csv_binding.identity.module_path, vec!["csv"]);

    let result = compile_package(&config, &entry);
    assert!(
        result.success(),
        "expected norma:csv transitive compile success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );

    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(output.code.contains("pub mod csv"));
    assert!(output.code.contains("pub mod chorda"));
    assert!(output.code.contains("pub(crate) fn discissa"));
    assert!(output.code.contains("pub(crate) fn nexa"));

    let layout = discover_build_layout(&entry).expect("layout");
    emit_generated_crate(&layout, &output.code, None).expect("emit generated crate");
    let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo toml");
    assert!(!cargo.contains("norma = { path = "));
}

#[test]
fn transitive_csv_solve_empty_input_returns_vacua() {
    let dir = test_temp_dir("transitive-csv-empty");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:csv" privata csv

incipit {
    fixum lista<lista<textus>> grid ← csv.solve("")
    nota grid.longitudo()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected empty solve success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );
}

#[test]
fn transitive_csv_solve_preserves_trailing_row() {
    let dir = test_temp_dir("transitive-csv-trailing");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        r#"
importa ex "norma:csv" privata csv

incipit {
    fixum lista<lista<textus>> grid ← csv.solve("a,b\n")
    nota grid.longitudo()
}
"#,
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &entry);
    assert!(
        result.success(),
        "expected trailing row solve success, got {:?}",
        transitive_test_diagnostic_facts(&result)
    );
}

#[test]
fn use_package_compiler_keeps_rust_fab_on_package_path() {
    let fab = Path::new("main.fab");
    assert!(use_package_compiler(Target::Rust, fab, false));
    assert!(use_package_compiler(Target::Scena, fab, false));
    assert!(use_package_compiler(Target::FmirText, fab, false));
    assert!(use_package_compiler(Target::Fmir, fab, false));
    assert!(use_package_compiler(Target::FmirBin, fab, false));
    assert!(!use_package_compiler(Target::WgslText, fab, false));
    assert!(!use_package_compiler(Target::LlvmText, fab, false));
    assert!(!use_package_compiler(Target::TypeScript, fab, false));
}

#[test]
fn use_package_compiler_from_args_honors_force_package_for_probe_targets() {
    let input = vec!["main.fab".to_owned()];
    assert!(use_package_compiler_from_args(
        Target::WgslText,
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
    fs::write(dir.join("src/main.fab"), "incipit { }\n").expect("entry");
    let package = analyze_package(&Config::default(), &dir).expect("analyze");
    let a = super::artifact_plan::plan_package(&package, Target::Rust);
    let b = super::artifact_plan::plan_package(&package, Target::Rust);
    assert!(a.supported);
    assert_eq!(a.to_debug_json().unwrap(), b.to_debug_json().unwrap());
    assert!(a.nodes.iter().any(|n| n.id.starts_with("rust:entry:")));
}

#[test]
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
        "functio double(numerus n) → numerus\n",
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
importa ex "libmath:math" privata math

incipit {
  fixum numerus n ← math.double(21)
  nota n
}
"#,
    )
    .expect("app entry");

    let report = verify_library_bindings(&lib, "rust").expect("library verifies");
    assert_eq!(report.bindings, 1);

    let result = compile_package(&Config::default(), &app);
    assert!(
        result.success(),
        "expected app compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.message.clone()))
            .collect::<Vec<_>>()
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

    let mut plan = package_rust_runtime_plan(&Config::default(), &app).expect("runtime plan");
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
        "functio double(numerus n) → numerus\n",
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
importa ex "libmath:math" privata math

incipit {
  fixum numerus n ← math.double(21)
  nota n
}
"#,
    )
    .expect("app entry");

    // Call while CWD is not the app root — relative lock paths must still resolve.
    let old = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&root).expect("chdir root");
    let deps = super::artifact_plan::native_library_deps(&app).expect("native deps");
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
fn library_genus_record_call_arg_emits_correct_type() {
    // Regression: package-path library Color {..} as call-arg must not emit as Euler.
    let app = test_temp_dir("lib-color-call-arg");
    fs::create_dir_all(app.join("src")).unwrap();
    fs::write(
        app.join("faber.toml"),
        r#"[package]
name = "color-call-arg"
version = "0.1.0"
[paths]
entry = "main.fab"
"#,
    )
    .unwrap();
    fs::write(
        app.join("src/main.fab"),
        r#"
importa ex "triga:triga" privata triga

incipit {
  fixum triga.Color midpoint ← triga.color_interpolata(
    triga.Color { r = 0.0 ∷ f32, g = 0.0 ∷ f32, b = 0.0 ∷ f32 },
    triga.Color { r = 1.0 ∷ f32, g = 0.5 ∷ f32, b = 0.25 ∷ f32 },
    0.5 ∷ f32
  )
  adfirma midpoint.r ≡ (0.5 ∷ f32), "color call-arg"
}
"#,
    )
    .unwrap();
    std::env::set_var(
        "FABER_LIBRARY_HOME",
        env!("CARGO_MANIFEST_DIR").to_owned() + "/..",
    );
    let result = compile_package(&Config::default(), &app);
    assert!(
        result.success(),
        "compile failed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust");
    };
    assert!(
        output.code.contains("crate::triga::Color {") || output.code.contains("triga::Color {"),
        "expected Color construct in emit, got:\n{}",
        output.code
    );
    assert!(
        !output
            .code
            .contains("color_interpolata(&crate::triga::Euler")
            && !output.code.contains("color_interpolata(&triga::Euler"),
        "Color call-arg must not emit as Euler:\n{}",
        output.code
    );
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
    fs::write(dir.join("src/main.fab"), "incipit { }\n").expect("entry");
    let result = compile_package(&Config::default().with_target(Target::Wasm), &dir);
    assert!(!result.success());
    assert!(result.diagnostics.iter().any(|d| diagnostic_has_issue(
        d,
        "package_target_unsupported"
    ) || diagnostic_has_issue(
        d,
        "package_target_assembly_pending"
    )));
}

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
@ operandus ceteri textus ignored
incipit argumenta args exitus 0 {
}
"#,
    )
    .expect("entry");

    let result = compile_package(&Config::default().with_target(Target::Go), &dir);
    assert!(
        result.success(),
        "go package compile failed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
    );
    let Some(Output::Go(output)) = result.output else {
        panic!("expected Go output");
    };
    assert!(
        output.code.contains("func main()") && output.code.contains("os.Exit(0)"),
        "expected Go CLI main with fixed exit:\n{}",
        output.code
    );

    let layout = discover_build_layout(&dir).expect("layout");
    let go_layout = super::GoBuildLayout::from_package(&layout);
    super::emit_go_module(&go_layout, &output.code, &[]).expect("emit go");
    let binary = super::invoke_go_build(&go_layout).expect("go build");
    assert!(binary.exists(), "binary missing at {}", binary.display());

    let status = Command::new(&binary)
        .args(["ignored", "args"])
        .status()
        .expect("run binary");
    assert_eq!(status.code(), Some(0), "true-style binary should exit 0");
}

#[test]
fn g6_go3_multi_module_package_is_deferred() {
    let dir = test_temp_dir("g6-go3-multi");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "g6-go3-multi"
version = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    // Local import pulls a second package unit → multi-module assembly path.
    fs::write(
        dir.join("src/helper.fab"),
        "functio identity(textus s) → textus {\n  redde s\n}\n",
    )
    .expect("helper");
    fs::write(
        dir.join("src/main.fab"),
        r#"
importa ex "./helper" privata identity

@ cli "tool"
@ operandus ceteri textus ignored
incipit argumenta args exitus 0 {
  fixum textus _ ← identity("x")
}
"#,
    )
    .expect("entry");

    let result = compile_package(&Config::default().with_target(Target::Go), &dir);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| diagnostic_has_issue(d, "package_go_multi_module_pending"))
            || (result.success() == false
                && result.diagnostics.iter().any(|d| {
                    d.message.contains("multi-module") || diagnostic_has_issue(d, "package_go_multi_module_pending")
                })),
        "expected multi-module pending or related fail, got success={} diags={:?}",
        result.success(),
        result
            .diagnostics
            .iter()
            .map(|d| (d.message.clone(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(!result.success());
}
