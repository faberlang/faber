//! ST2-2.10 tests: the single-file emit route retries once through the
//! package analysis when a `comptime` initializer reached an import
//! (`praefixum_import_unavailable`), and on nothing else.
//!
//! Assertions are on structured identities (`code` / `issue` / args) and on
//! emitted program text, never on diagnostic prose.

use std::path::Path;

use radix::module::codegen::{OutputMode, Target};
use radix::module::locale::LocalePack;
use radix::services::{DiagnosticMode, EmitCommand};

use super::emit;

const CONSTS: &str = "+++\nlocale = \"en\"\n+++\n\n@ public\nconst int BASE = 5\n\n@ public\nfn seven() \u{2192} int { return 7 }\n";

const CONSTS_PRINTING: &str = "+++\nlocale = \"en\"\n+++\n\n@ public\nfn shout() \u{2192} int {\n    print 1\n    return 7\n}\n";

#[cfg(feature = "hir-ts")]
const COMPTIME_CONSUMER: &str = "+++\nlocale = \"en\"\n+++\n\nimport from \"./consts\" consts\n\nconst int G = comptime {\n    return consts.seven()\n}\n\nmain {\n    print G\n}\n";

const FORBIDDEN_CONSUMER: &str = "+++\nlocale = \"en\"\n+++\n\nimport from \"./consts\" consts\n\nconst int G = comptime {\n    return consts.shout()\n}\n\nmain {\n    print G\n}\n";

#[cfg(all(
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-python",
    feature = "mir-wasm"
))]
const PLAIN_CONSUMER: &str = "+++\nlocale = \"en\"\n+++\n\nimport from \"./consts\" consts\n\nconst int D = consts.BASE * 2\n\nmain {\n    print D\n}\n";

const UNDEFINED_NAME: &str = "+++\nlocale = \"en\"\n+++\n\nmain {\n    print nomen_inexistens\n}\n";

fn emit_command(input: &Path, target: Target) -> EmitCommand {
    EmitCommand {
        input: vec![input.display().to_string()],
        package: false,
        target,
        reflection: false,
        #[cfg(feature = "mir-llvm")]
        cuda_descriptor: None,
        output: None,
        diagnostic_mode: DiagnosticMode::Normal,
        diagnostics_locale_pack: None,
        locale: None,
        diagnostics_locale: None,
        output_mode: OutputMode::Application,
        module_name: None,
    }
}

/// The reader pack `cmd_emit_with_locale` would resolve for the command.
fn code_pack(command: &EmitCommand) -> Option<LocalePack> {
    faber_cli::package::locale_pack_for_emit_with_diagnostic(
        &command.input,
        command.locale.as_deref(),
    )
    .expect("locale pack resolves for a file input")
}

/// The single-file result `cmd_emit_with_locale` computes before its retry.
fn single_file_result(command: &EmitCommand, pack: Option<&LocalePack>) -> radix::CompileResult {
    radix::services::compile_cli_input_with_locale_pack(
        &command.input,
        command.package,
        command.target,
        pack,
        command.output_mode,
        command.module_name.as_deref(),
        None,
    )
}

fn issues(result: &radix::CompileResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.issue().map(str::to_owned))
        .collect()
}

fn fixture(dir: &Path, name: &str, source: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, source).expect("write fixture");
    path
}

#[cfg(all(feature = "hir-ts", feature = "hir-go"))]
#[test]
fn emit_retry_folds_imported_comptime_for_ts_and_go() {
    let dir = tempfile::tempdir().expect("temp dir");
    fixture(dir.path(), "consts.fab", CONSTS);
    let consumer = fixture(dir.path(), "c.fab", COMPTIME_CONSUMER);

    for target in [Target::HirTypeScript, Target::HirGo] {
        let command = emit_command(&consumer, target);
        let pack = code_pack(&command);
        let single = single_file_result(&command, pack.as_ref());
        assert!(
            single.output.is_none(),
            "{target}: expected the single-file refusal"
        );
        assert!(
            issues(&single)
                .iter()
                .any(|issue| issue == "praefixum_import_unavailable"),
            "{target}: expected the structured import refusal, got {:?}",
            issues(&single)
        );

        let retried = emit::retry_single_file_via_package_analysis(&command, pack.as_ref(), single);
        let retry_issues = issues(&retried);
        let output = retried
            .output
            .unwrap_or_else(|| panic!("{target}: retry emitted nothing: {retry_issues:?}"));
        let code = radix::services::output_code(output);
        assert!(
            code.contains('7'),
            "{target}: folded constant missing from output:\n{code}"
        );
        assert!(
            !code.contains("seven()"),
            "{target}: output still calls seven:\n{code}"
        );
    }
}

#[test]
fn emit_retry_skips_package_analysis_for_unrelated_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bad = fixture(dir.path(), "bad.fab", UNDEFINED_NAME);

    let command = emit_command(&bad, Target::HirTypeScript);
    let pack = code_pack(&command);
    let single = single_file_result(&command, pack.as_ref());
    assert!(single.output.is_none());
    let expected = issues(&single);
    assert!(
        !expected.is_empty(),
        "expected single-file diagnostics from the undefined name"
    );

    emit::reset_package_analysis_entries();
    let retried = emit::retry_single_file_via_package_analysis(&command, pack.as_ref(), single);
    assert_eq!(
        emit::package_analysis_entries(),
        0,
        "an unrelated diagnostic must not enter the package analysis"
    );
    assert!(retried.output.is_none());
    assert_eq!(
        issues(&retried),
        expected,
        "the unrelated error is reported once, untouched by the retry"
    );
}

#[cfg(all(
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-python",
    feature = "mir-wasm"
))]
#[test]
fn emit_retry_matches_single_file_output_for_plain_imports() {
    let dir = tempfile::tempdir().expect("temp dir");
    fixture(dir.path(), "consts.fab", CONSTS);
    let plain = fixture(dir.path(), "a.fab", PLAIN_CONSUMER);

    for target in [
        Target::HirTypeScript,
        Target::HirGo,
        Target::HirPython,
        Target::MirWasm,
    ] {
        for locale in [None, Some("la")] {
            for output_mode in [OutputMode::Application, OutputMode::Library] {
                for module_name in [None, Some("paritas")] {
                    let mut command = emit_command(&plain, target);
                    command.locale = locale.map(str::to_owned);
                    command.output_mode = output_mode;
                    command.module_name = module_name.map(str::to_owned);
                    let pack = code_pack(&command);

                    let single = single_file_result(&command, pack.as_ref());
                    let single_issues = issues(&single);
                    let single_code = single.output.map(radix::services::output_code);
                    let package =
                        emit::emit_through_package_analysis(&command, pack.as_ref(), &plain);
                    let package_issues = issues(&package);
                    let package_code = package.output.map(radix::services::output_code);

                    let context = format!(
                        "{target} locale={locale:?} mode={output_mode:?} module={module_name:?}"
                    );
                    match (single_code, package_code) {
                        (Some(single_code), Some(package_code)) => {
                            assert_eq!(single_code, package_code, "parity for {context}");
                        }
                        (None, None) => {
                            assert_eq!(
                                single_issues, package_issues,
                                "refusal parity for {context}"
                            );
                        }
                        (single_code, package_code) => {
                            panic!(
                                "parity shape mismatch for {context}: single={:?} package={:?}",
                                single_code.is_some(),
                                package_code.is_some()
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn emit_retry_reports_forbidden_effect_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    fixture(dir.path(), "consts.fab", CONSTS_PRINTING);
    let consumer = fixture(dir.path(), "c.fab", FORBIDDEN_CONSUMER);

    let command = emit_command(&consumer, Target::HirTypeScript);
    let pack = code_pack(&command);
    let single = single_file_result(&command, pack.as_ref());
    assert!(single.output.is_none());
    assert!(
        issues(&single)
            .iter()
            .any(|issue| issue == "praefixum_import_unavailable"),
        "expected the single-file import refusal, got {:?}",
        issues(&single)
    );

    emit::reset_package_analysis_entries();
    let retried = emit::retry_single_file_via_package_analysis(&command, pack.as_ref(), single);
    assert_eq!(
        emit::package_analysis_entries(),
        1,
        "the retry runs exactly once"
    );
    assert!(retried.output.is_none());
    let forbidden = retried
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.issue() == Some("praefixum_forbidden"))
        .count();
    assert_eq!(
        forbidden,
        1,
        "expected one forbidden-effect diagnostic: {:?}",
        issues(&retried)
    );
}

#[cfg(feature = "hir-ts")]
#[test]
fn package_analysis_bindings_skip_non_wgsl_targets() {
    let dir = tempfile::tempdir().expect("temp dir");
    fixture(dir.path(), "consts.fab", CONSTS);
    let consumer = fixture(dir.path(), "c.fab", COMPTIME_CONSUMER);

    let command = emit_command(&consumer, Target::HirTypeScript);
    let pack = code_pack(&command);
    let single = single_file_result(&command, pack.as_ref());
    assert!(
        single.output.is_none(),
        "expected the single-file refusal before the retry"
    );

    let retried = emit::retry_single_file_via_package_analysis(&command, pack.as_ref(), single);
    assert!(
        retried.output.is_some(),
        "a non-WGSL target must keep emitting through the retry: {:?}",
        issues(&retried)
    );
}

#[test]
fn package_analysis_config_leaves_file_locale_declarations_in_charge() {
    let dir = tempfile::tempdir().expect("temp dir");
    let input = fixture(dir.path(), "c.fab", CONSTS);
    let command = emit_command(&input, Target::HirTypeScript);
    let pack = code_pack(&command).unwrap_or_else(radix::module::locale::latin_locale_pack);

    let config = emit::package_analysis_config(&command, Some(&pack));
    assert!(
        !config.locale_pack_explicit,
        "the retry config must leave each file's frontmatter locale in charge"
    );
    assert!(
        config.locale_pack.is_some(),
        "the retry config still carries the resolved pack as the fallback"
    );
    assert!(
        emit::package_analysis_config(&command, None)
            .locale_pack
            .is_none(),
        "without a pack there is no reader pack to carry"
    );
}

#[test]
fn library_root_of_interface_finds_the_manifest_directory() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().join("provider");
    let source = root.join("src");
    std::fs::create_dir_all(&source).expect("create source dir");
    std::fs::write(root.join("faber.toml"), "[package]\n").expect("write manifest");
    let interface = source.join("x.fab");
    std::fs::write(&interface, "").expect("write interface");

    assert_eq!(
        emit::library_root_of_interface(&interface),
        Some(root),
        "the manifest directory is the library package root"
    );
}

#[test]
fn merge_library_target_bindings_on_no_roots_is_empty() {
    let (bindings, diagnostics) = emit::merge_library_target_bindings(
        &std::collections::BTreeSet::new(),
        &Target::MirWgsl.to_string(),
    );
    assert!(bindings.functions.is_empty(), "no roots bind no functions");
    assert!(bindings.types.is_empty(), "no roots bind no types");
    assert!(diagnostics.is_empty(), "no roots load no diagnostics");
}

// ── single-file emit honours a declared frontmatter locale on an install ──
//
// The emit lane's compile entries used to mint their config without any pack
// source for the frontmatter locale: an installed binary (no workspace
// `locale/` tree) failed to resolve `+++ locale = "…" +++` and read the
// source as English. The lane now hands the compile its locale base — the
// embedded reader packs — and these tests model the install by clearing the
// discovered root from that exact base.

/// The emit lane's locale base with no discoverable workspace root: the
/// installed-binary condition, where only the embedded packs remain.
fn installed_locale_base() -> radix::driver::Config {
    let mut base = emit::emit_locale_base();
    base.locale_root = None;
    base
}

const LA_IDENTITY: &str = "+++\nlocale = \"la\"\n+++\n\n@ publica\nfunctio identitas(bivalens valor) → bivalens {\n    redde valor\n}\n";

const LA_HADAMARD: &str = "+++\nlocale = \"la\"\n+++\n\n@ nucleum\n@ publica\nfunctio hadamard(tf32[256] sinistra, tf32[256] dextera) → tf32[256] {\n    redde sinistra ⊙ dextera\n}\n";

#[cfg(feature = "mir-llvm")]
#[test]
fn emit_single_file_honours_frontmatter_locale_without_pack_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let entry = fixture(dir.path(), "main.fab", LA_IDENTITY);
    let command = emit_command(&entry, Target::MirLlvm);
    let pack = code_pack(&command);

    let result = radix::services::compile_cli_input_with_locale_pack(
        &command.input,
        command.package,
        command.target,
        pack.as_ref(),
        command.output_mode,
        command.module_name.as_deref(),
        Some(&installed_locale_base()),
    );

    assert!(
        result.success(),
        "the installed binary must read the declared Latin locale, diagnostics: {:?}",
        result.diagnostics
    );
    let code = radix::services::output_code(result.output.expect("llvm-text output"));
    assert!(
        code.contains("identitas"),
        "the emitted IR must name the Latin function:\n{code}"
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_wgsl_route_honours_frontmatter_locale_without_pack_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let entry = fixture(dir.path(), "main.fab", LA_HADAMARD);
    let command = emit_command(&entry, Target::MirWgsl);
    let pack = code_pack(&command);
    let resolve = |_roots: &std::collections::BTreeSet<std::path::PathBuf>| {
        radix::driver::TargetBindings::default()
    };

    let result = radix::services::compile_cli_input_with_library_bindings(
        &command.input,
        command.package,
        command.target,
        pack.as_ref(),
        command.output_mode,
        command.module_name.as_deref(),
        &resolve,
        Some(&installed_locale_base()),
    );

    assert!(
        result.success(),
        "the WGSL route must read the declared Latin locale, diagnostics: {:?}",
        result.diagnostics
    );
    let code = radix::services::output_code(result.output.expect("wgsl-text output"));
    assert!(
        code.contains("hadamard"),
        "the emitted WGSL must name the Latin kernel:\n{code}"
    );
}
