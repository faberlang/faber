//! The single-file read lanes (`faber check`, `lex`, `parse`, `hir`,
//! `cli-ir`, `verify`) install the product's locale policy — the embedded
//! reader packs — at their service call sites, so a Latin
//! frontmatter file reads under its declared locale on an installed binary
//! with no workspace `locale/` tree beside it.
//!
//! Each test takes the exact config the command installs and clears the
//! discovered workspace root: the installed-binary condition.

use super::single_file_locale_config;

const LA_SOURCE: &str = "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio salve() { }\n";

/// The lane's locale config with no discoverable workspace root.
fn installed_locale_config(input: &[String]) -> radix::module::Config {
    let mut config = single_file_locale_config(input);
    config.locale_root = None;
    config
}

#[test]
fn check_lane_reads_declared_locale_without_workspace_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("main.fab");
    std::fs::write(&file, LA_SOURCE).expect("write source");
    let input = vec![file.display().to_string()];

    let result = radix::services::check_diagnostics_for_source_with_config(
        &file.display().to_string(),
        LA_SOURCE,
        false,
        None,
        None,
        &installed_locale_config(&input),
    );
    assert!(
        result.success,
        "a Latin frontmatter file must check on an installed binary: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn lex_and_parse_lanes_read_declared_locale_without_workspace_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("main.fab");
    std::fs::write(&file, LA_SOURCE).expect("write source");
    let name = file.display().to_string();
    let input = vec![name.clone()];
    let config = installed_locale_config(&input);

    let lexed = radix::services::lex_inspection_for_source_with_config(&name, LA_SOURCE, &config);
    assert!(lexed.success, "errors: {:?}", lexed.errors);
    assert!(
        lexed.text.contains("\"keyword\": \"functio\""),
        "the Latin `functio` must lex as a keyword: {}",
        lexed.text
    );

    let parsed =
        radix::services::parse_inspection_for_source_with_config(&name, LA_SOURCE, &config);
    assert!(parsed.success, "errors: {:?}", parsed.errors);
}

#[test]
fn hir_and_cli_ir_lanes_read_declared_locale_without_workspace_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("main.fab");
    std::fs::write(&file, LA_SOURCE).expect("write source");
    let name = file.display().to_string();
    let input = vec![name.clone()];
    let config = installed_locale_config(&input);

    let hir = radix::services::hir_inspection_for_source_with_config(&name, LA_SOURCE, &config);
    assert!(hir.success, "errors: {:?}", hir.errors);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&hir.text).unwrap()["items"],
        1
    );

    let cli_source = "+++\nlocale = \"la\"\n+++\nincipit {\n    nota \"salve\"\n}\n";
    let cli = radix::services::cli_ir_inspection_for_source_with_config(&name, cli_source, &config);
    assert!(cli.success, "errors: {:?}", cli.errors);
}

#[test]
fn verify_lane_reads_declared_locale_without_workspace_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("main.fab");
    std::fs::write(&file, LA_SOURCE).expect("write source");
    let input = vec![file.display().to_string()];

    let result = radix::services::verify_source_with_config(
        &file.display().to_string(),
        LA_SOURCE,
        None,
        &installed_locale_config(&input),
    );
    // A clean Latin read under the declared locale produces no diagnostics at
    // all — not merely none of the lost-locale codes — and the aspect
    // machinery still ran (its coverage report is present; without a Lean
    // checker configured the entries report `ToolchainMissing`, which the
    // lane's success gate accounts for separately).
    assert!(
        result.diagnostics.is_empty(),
        "verify must read the Latin source cleanly under its declared locale: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        result.coverage.is_some(),
        "verify must still produce its aspect coverage report"
    );
}
