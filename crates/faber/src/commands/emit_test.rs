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

const COMPTIME_CONSUMER: &str = "+++\nlocale = \"en\"\n+++\n\nimport from \"./consts\" consts\n\nconst int G = comptime {\n    return consts.seven()\n}\n\nmain {\n    print G\n}\n";

const FORBIDDEN_CONSUMER: &str = "+++\nlocale = \"en\"\n+++\n\nimport from \"./consts\" consts\n\nconst int G = comptime {\n    return consts.shout()\n}\n\nmain {\n    print G\n}\n";

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
fn merge_library_target_bindings_on_no_roots_is_empty() {
    let (bindings, diagnostics) = emit::merge_library_target_bindings(
        &std::collections::BTreeSet::new(),
        &Target::MirWgsl.to_string(),
    );
    assert!(bindings.functions.is_empty(), "no roots bind no functions");
    assert!(bindings.types.is_empty(), "no roots bind no types");
    assert!(diagnostics.is_empty(), "no roots load no diagnostics");
}
