use crate::cli::{Cli, FaberCliTarget};
use clap::{CommandFactory, Parser};

#[test]
fn cli_long_help_includes_llm_guidance_and_output_contract() {
    let help = Cli::command().render_long_help().to_string();

    assert!(help.contains("LLM Guidance"));
    assert!(help.contains("Output contract"));
    assert!(help.contains("faber init"));
    assert!(help.contains("faber explain"));
}

#[test]
fn cli_parses_c_one_liner_without_subcommand() {
    let cli = Cli::try_parse_from(["faber", "-c", "incipit { nota 1 }"]).expect("parse -c");
    assert!(cli.command.is_none());
    let source = cli.eval_source.expect("eval source");
    assert!(source.contains("incipit"));
}

#[test]
fn cli_parses_c_one_liner_forwarded_args_after_double_dash() {
    let cli = Cli::try_parse_from(["faber", "-c", "incipit { nota 1 }", "--", "--flag", "value"])
        .expect("parse -c forwarded args");
    assert!(cli.command.is_none());
    assert_eq!(cli.eval_args, vec!["--flag".to_owned(), "value".to_owned()]);
}

#[test]
fn cli_parses_repl_subcommand() {
    let cli = Cli::try_parse_from(["faber", "repl"]).expect("parse repl");
    assert!(cli.eval_source.is_none());
    assert!(matches!(cli.command, Some(crate::cli::Command::Repl(_))));
}

#[test]
fn cli_parses_repl_subcommand_with_forwarded_args() {
    let cli = Cli::try_parse_from(["faber", "repl", "--", "--flag", "value"])
        .expect("parse repl forwarded args");
    let Some(crate::cli::Command::Repl(args)) = cli.command else {
        panic!("expected repl subcommand");
    };
    assert_eq!(args.args, vec!["--flag".to_owned(), "value".to_owned()]);
}

#[test]
fn cli_parses_targets_subcommand() {
    let cli = Cli::try_parse_from(["faber", "targets"]).expect("parse targets");
    assert!(cli.eval_source.is_none());
    assert!(matches!(cli.command, Some(crate::cli::Command::Targets)));
}

#[test]
fn cli_parses_script_subcommand_with_forwarded_args() {
    let cli = Cli::try_parse_from(["faber", "script", "pkg", "--", "--flag", "value"])
        .expect("parse script");
    let Some(crate::cli::Command::Script(args)) = cli.command else {
        panic!("expected script subcommand");
    };
    assert_eq!(args.path, std::path::PathBuf::from("pkg"));
    assert_eq!(args.args, vec!["--flag".to_owned(), "value".to_owned()]);
}

#[test]
fn cli_parses_test_subcommand_selection_and_harness_flags() {
    let cli = Cli::try_parse_from([
        "faber",
        "test",
        "pkg",
        "smoke",
        "--name",
        "suite_case",
        "--suite",
        "suite/path",
        "--tag",
        "slow",
        "--exact",
        "--nocapture",
        "--test-threads",
        "4",
        "--include-ignored",
    ])
    .expect("parse test");
    let Some(crate::cli::Command::Test(args)) = cli.command else {
        panic!("expected test subcommand");
    };
    assert_eq!(args.path, std::path::PathBuf::from("pkg"));
    assert_eq!(args.filter.as_deref(), Some("smoke"));
    assert_eq!(args.name.as_deref(), Some("suite_case"));
    assert_eq!(args.suite.as_deref(), Some("suite/path"));
    assert_eq!(args.tag.as_deref(), Some("slow"));
    assert!(args.exact);
    assert!(args.nocapture);
    assert_eq!(args.test_threads, Some(4));
    assert!(!args.ignored);
    assert!(args.include_ignored);
}

#[test]
fn cli_parses_hidden_fmir_run_with_forwarded_args() {
    let cli = Cli::try_parse_from([
        "faber",
        "__fmir-run",
        "target/faber-mir/exe/run",
        "--",
        "--flag",
        "value",
    ])
    .expect("parse hidden fmir runner");
    let Some(crate::cli::Command::FmirRun(args)) = cli.command else {
        panic!("expected hidden fmir runner subcommand");
    };
    assert_eq!(
        args.image,
        std::path::PathBuf::from("target/faber-mir/exe/run")
    );
    assert_eq!(args.args, vec!["--flag".to_owned(), "value".to_owned()]);
}

#[test]
fn cli_parses_emit_wgsl_text_target() {
    let cli =
        Cli::try_parse_from(["faber", "emit", "-t", "wgsl-text", "main.fab"]).expect("parse emit");
    let Some(crate::cli::Command::Emit(args)) = cli.command else {
        panic!("expected emit subcommand");
    };
    assert_eq!(args.target, FaberCliTarget::WgslText);
    assert_eq!(args.input, vec!["main.fab"]);
}

#[test]
fn cli_parses_legacy_ir_alias_subcommands() {
    let lex = Cli::try_parse_from(["faber", "lex", "main.fab"]).expect("parse lex");
    let Some(crate::cli::Command::Lex(args)) = lex.command else {
        panic!("expected lex subcommand");
    };
    assert_eq!(args.input, vec!["main.fab"]);

    let parse = Cli::try_parse_from(["faber", "parse", "main.fab"]).expect("parse parse");
    let Some(crate::cli::Command::Parse(args)) = parse.command else {
        panic!("expected parse subcommand");
    };
    assert_eq!(args.input, vec!["main.fab"]);

    let hir = Cli::try_parse_from(["faber", "hir", "main.fab"]).expect("parse hir");
    let Some(crate::cli::Command::Hir(args)) = hir.command else {
        panic!("expected hir subcommand");
    };
    assert_eq!(args.input, vec!["main.fab"]);

    let cli_ir = Cli::try_parse_from(["faber", "cli-ir", "main.fab"]).expect("parse cli-ir");
    let Some(crate::cli::Command::CliIr(args)) = cli_ir.command else {
        panic!("expected cli-ir subcommand");
    };
    assert_eq!(args.input, vec!["main.fab"]);
}

#[test]
fn cli_parses_scena_target_for_build_and_run() {
    let build = Cli::try_parse_from(["faber", "build", "--target", "scena", "pkg"])
        .expect("parse build scena target");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, Some(radix::tool::CliTarget::Scena));
    assert_eq!(args.input, "pkg");

    let run = Cli::try_parse_from(["faber", "run", "--target", "scena", "pkg", "--", "Ian"])
        .expect("parse run scena target");
    let Some(crate::cli::Command::Run(args)) = run.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.target, radix::tool::CliTarget::Scena);
    assert_eq!(args.path, std::path::PathBuf::from("pkg"));
    assert_eq!(args.args, vec!["Ian".to_owned()]);
}

#[test]
fn cli_run_defaults_to_current_directory_and_rust_target() {
    let run = Cli::try_parse_from(["faber", "run"]).expect("parse run defaults");
    let Some(crate::cli::Command::Run(args)) = run.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.path, std::path::PathBuf::from("."));
    assert_eq!(args.target, radix::tool::CliTarget::Rust);
    assert!(!args.release);
    assert!(!args.interpret);
    assert!(!args.compile);
    assert!(args.args.is_empty());
}

#[test]
fn cli_rejects_conflicting_run_execution_modes() {
    let error = Cli::try_parse_from(["faber", "run", "--interpret", "--compile", "pkg"])
        .expect_err("run execution mode conflict");
    let rendered = error.to_string();
    assert!(rendered.contains("--interpret"));
    assert!(rendered.contains("--compile"));
}

#[test]
fn cli_leaves_build_target_unset_when_omitted() {
    let build = Cli::try_parse_from(["faber", "build", "pkg"]).expect("parse build");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, None);
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_build_output_and_mode_flags() {
    let build = Cli::try_parse_from([
        "faber",
        "build",
        "--out-dir",
        "dist",
        "--package",
        "--release",
        "--format",
        "--linter",
        "pkg",
    ])
    .expect("parse build flags");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.out_dir, std::path::PathBuf::from("dist"));
    assert!(args.package);
    assert!(args.release);
    assert!(args.format);
    assert!(args.linter);
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_fmir_text_target_for_build() {
    let build = Cli::try_parse_from(["faber", "build", "--target", "fmir-text", "pkg"])
        .expect("parse build fmir-text target");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, Some(radix::tool::CliTarget::FmirText));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_fmir_target_for_build() {
    let build = Cli::try_parse_from(["faber", "build", "--target", "fmir", "pkg"])
        .expect("parse build fmir target");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, Some(radix::tool::CliTarget::Fmir));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_fmir_bin_target_for_build_and_run() {
    let build = Cli::try_parse_from(["faber", "build", "--target", "fmir-bin", "pkg"])
        .expect("parse build fmir-bin target");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, Some(radix::tool::CliTarget::FmirBin));
    assert_eq!(args.input, "pkg");

    let run = Cli::try_parse_from(["faber", "run", "--target", "fmir-bin", "pkg", "--", "Ian"])
        .expect("parse run fmir-bin target");
    let Some(crate::cli::Command::Run(args)) = run.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.target, radix::tool::CliTarget::FmirBin);
    assert_eq!(args.path, std::path::PathBuf::from("pkg"));
    assert_eq!(args.args, vec!["Ian".to_owned()]);
}

#[test]
fn cli_parses_reader_locale_on_check_emit_build_and_format() {
    let check = Cli::try_parse_from(["faber", "check", "--reader-locale", "zh-Hans", "main.fab"])
        .expect("parse check reader locale");
    let Some(crate::cli::Command::Check(args)) = check.command else {
        panic!("expected check subcommand");
    };
    assert_eq!(args.reader_locale.as_deref(), Some("zh-Hans"));

    let emit = Cli::try_parse_from([
        "faber",
        "emit",
        "--reader-locale",
        "zh-Hans",
        "-t",
        "rust",
        "main.fab",
    ])
    .expect("parse emit reader locale");
    let Some(crate::cli::Command::Emit(args)) = emit.command else {
        panic!("expected emit subcommand");
    };
    assert_eq!(args.reader_locale.as_deref(), Some("zh-Hans"));

    let build = Cli::try_parse_from(["faber", "build", "--reader-locale", "zh-Hans", "main.fab"])
        .expect("parse build reader locale");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.reader_locale.as_deref(), Some("zh-Hans"));

    let format = Cli::try_parse_from([
        "faber",
        "format",
        "--canonical",
        "--reader-locale",
        "zh-Hans",
        "main.fab",
    ])
    .expect("parse format reader locale");
    let Some(crate::cli::Command::Format(args)) = format.command else {
        panic!("expected format subcommand");
    };
    assert_eq!(args.reader_locale.as_deref(), Some("zh-Hans"));
}

#[test]
fn cli_parses_check_flags_and_multiple_inputs() {
    let check = Cli::try_parse_from([
        "faber",
        "check",
        "--diagnostics",
        "--permissive",
        "--package",
        "main.fab",
        "other.fab",
    ])
    .expect("parse check flags");
    let Some(crate::cli::Command::Check(args)) = check.command else {
        panic!("expected check subcommand");
    };
    assert!(args.diagnostics);
    assert!(args.permissive);
    assert!(args.package);
    assert_eq!(args.input, vec!["main.fab", "other.fab"]);
}

#[test]
fn cli_rejects_conflicting_format_output_modes() {
    let error = Cli::try_parse_from(["faber", "format", "--check", "--stdout", "main.fab"])
        .expect_err("format output mode conflict");
    let rendered = error.to_string();
    assert!(rendered.contains("--check"));
    assert!(rendered.contains("--stdout"));
}

#[test]
fn cli_parses_reader_locale_on_explain() {
    let explain = Cli::try_parse_from_validated([
        "faber",
        "explain",
        "--reader-locale",
        "zh-Hans",
        "SEM010.initializer_annotation_mismatch",
    ])
    .expect("parse explain reader locale");
    let Some(crate::cli::Command::Explain(args)) = explain.command else {
        panic!("expected explain subcommand");
    };
    assert_eq!(args.reader_locale.as_deref(), Some("zh-Hans"));
    assert_eq!(
        args.term.as_deref(),
        Some("SEM010.initializer_annotation_mismatch")
    );
}

#[test]
fn cli_parses_explain_query_modes() {
    let json = Cli::try_parse_from_validated(["faber", "explain", "--json", "nihil"])
        .expect("parse explain json");
    let Some(crate::cli::Command::Explain(json_args)) = json.command else {
        panic!("expected explain subcommand");
    };
    assert!(json_args.json);
    assert_eq!(json_args.term.as_deref(), Some("nihil"));

    let search = Cli::try_parse_from_validated(["faber", "explain", "--search", "host"])
        .expect("parse explain search");
    let Some(crate::cli::Command::Explain(search_args)) = search.command else {
        panic!("expected explain subcommand");
    };
    assert_eq!(search_args.search.as_deref(), Some("host"));
    assert!(search_args.term.is_none());

    let list =
        Cli::try_parse_from_validated(["faber", "explain", "--list"]).expect("parse explain list");
    let Some(crate::cli::Command::Explain(list_args)) = list.command else {
        panic!("expected explain subcommand");
    };
    assert!(list_args.list);
    assert!(list_args.term.is_none());

    let category = Cli::try_parse_from_validated(["faber", "explain", "--category", "diagnostics"])
        .expect("parse explain category");
    let Some(crate::cli::Command::Explain(category_args)) = category.command else {
        panic!("expected explain subcommand");
    };
    assert_eq!(category_args.category.as_deref(), Some("diagnostics"));
    assert!(category_args.term.is_none());
}

#[test]
fn cli_rejects_conflicting_explain_query_modes() {
    let mixed = Cli::try_parse_from_validated(["faber", "explain", "--list", "nihil"])
        .expect_err("list and term should conflict");
    let mixed_rendered = mixed.to_string();
    assert!(mixed_rendered.contains("--list"));

    let search_json =
        Cli::try_parse_from_validated(["faber", "explain", "--search", "host", "--json"])
            .expect_err("search and json should conflict");
    let search_json_rendered = search_json.to_string();
    assert!(search_json_rendered.contains("--search"));
    assert!(search_json_rendered.contains("--json"));

    let search_category = Cli::try_parse_from_validated([
        "faber",
        "explain",
        "--search",
        "host",
        "--category",
        "diagnostics",
    ])
    .expect_err("search and category should conflict");
    let search_category_rendered = search_category.to_string();
    assert!(search_category_rendered.contains("--search"));
    assert!(search_category_rendered.contains("--category"));

    let list_reader_locale =
        Cli::try_parse_from_validated(["faber", "explain", "--list", "--reader-locale", "la"])
            .expect_err("list and reader locale should conflict");
    let list_reader_locale_rendered = list_reader_locale.to_string();
    assert!(list_reader_locale_rendered.contains("--list"));
    assert!(list_reader_locale_rendered.contains("--reader-locale"));

    let search_reader_locale = Cli::try_parse_from_validated([
        "faber",
        "explain",
        "--search",
        "host",
        "--reader-locale",
        "la",
    ])
    .expect_err("search and reader locale should conflict");
    let search_reader_locale_rendered = search_reader_locale.to_string();
    assert!(search_reader_locale_rendered.contains("--search"));
    assert!(search_reader_locale_rendered.contains("--reader-locale"));

    let category_reader_locale = Cli::try_parse_from_validated([
        "faber",
        "explain",
        "--category",
        "diagnostics",
        "--reader-locale",
        "la",
    ])
    .expect_err("category and reader locale should conflict");
    let category_reader_locale_rendered = category_reader_locale.to_string();
    assert!(category_reader_locale_rendered.contains("--category"));
    assert!(category_reader_locale_rendered.contains("--reader-locale"));
}

#[test]
fn cli_rejects_explain_json_without_term() {
    let error = Cli::try_parse_from_validated(["faber", "explain", "--json"])
        .expect_err("json requires a term");
    let rendered = error.to_string();
    assert!(rendered.contains("--json"));
    assert!(rendered.contains("<TERM>") || rendered.contains("<term>"));
}

#[test]
fn cli_rejects_explain_reader_locale_without_term() {
    let error = Cli::try_parse_from_validated(["faber", "explain", "--reader-locale", "la"])
        .expect_err("reader locale requires a term");
    let rendered = error.to_string();
    assert!(rendered.contains("required arguments were not provided"));
    assert!(rendered.contains("<TERM>") || rendered.contains("<term>"));
}

#[test]
fn cli_parses_verify_subcommand() {
    let cli = Cli::try_parse_from(["faber", "verify", "main.fab"]).expect("parse verify");
    let Some(crate::cli::Command::Verify(args)) = cli.command else {
        panic!("expected verify subcommand");
    };
    assert!(!args.package);
    assert_eq!(args.input, vec!["main.fab"]);
}

#[test]
fn cli_parses_verify_library_subcommand() {
    let cli = Cli::try_parse_from(["faber", "verify-library", "--target", "rust", "sqlite"])
        .expect("parse verify-library");
    let Some(crate::cli::Command::VerifyLibrary(args)) = cli.command else {
        panic!("expected verify-library subcommand");
    };
    assert_eq!(args.target, "rust");
    assert_eq!(args.input, std::path::PathBuf::from("sqlite"));
}

#[test]
fn cli_parses_install_subcommand() {
    let cli = Cli::try_parse_from(["faber", "install", "norma"]).expect("parse install");
    let Some(crate::cli::Command::Install(args)) = cli.command else {
        panic!("expected install subcommand");
    };
    assert_eq!(args.library, "norma");
}

#[test]
fn cli_init_defaults_to_current_directory() {
    let cli = Cli::try_parse_from(["faber", "init"]).expect("parse init");
    let Some(crate::cli::Command::Init(args)) = cli.command else {
        panic!("expected init subcommand");
    };
    assert_eq!(args.path, std::path::PathBuf::from("."));
}

#[test]
fn cli_script_and_test_default_to_current_directory() {
    let script = Cli::try_parse_from(["faber", "script"]).expect("parse script defaults");
    let Some(crate::cli::Command::Script(script_args)) = script.command else {
        panic!("expected script subcommand");
    };
    assert_eq!(script_args.path, std::path::PathBuf::from("."));
    assert!(script_args.args.is_empty());

    let test = Cli::try_parse_from(["faber", "test"]).expect("parse test defaults");
    let Some(crate::cli::Command::Test(test_args)) = test.command else {
        panic!("expected test subcommand");
    };
    assert_eq!(test_args.path, std::path::PathBuf::from("."));
    assert!(test_args.filter.is_none());
    assert!(!test_args.exact);
    assert!(!test_args.nocapture);
    assert!(!test_args.ignored);
    assert!(!test_args.include_ignored);
}

#[test]
fn cli_verify_library_uses_default_rust_target() {
    let cli = Cli::try_parse_from(["faber", "verify-library", "sqlite"])
        .expect("parse verify-library defaults");
    let Some(crate::cli::Command::VerifyLibrary(args)) = cli.command else {
        panic!("expected verify-library subcommand");
    };
    assert_eq!(args.target, "rust");
    assert_eq!(args.input, std::path::PathBuf::from("sqlite"));
}

#[test]
fn cli_test_rejects_conflicting_ignored_modes() {
    let error = Cli::try_parse_from(["faber", "test", "pkg", "--ignored", "--include-ignored"])
        .expect_err("test ignored mode conflict");
    let rendered = error.to_string();
    assert!(rendered.contains("--ignored"));
    assert!(rendered.contains("--include-ignored"));
}

#[test]
fn cli_parses_host_manifest_json_subcommand() {
    let cli =
        Cli::try_parse_from(["faber", "host", "manifest", "--json"]).expect("parse host manifest");
    let Some(crate::cli::Command::Host(args)) = cli.command else {
        panic!("expected host subcommand");
    };
    assert!(matches!(
        args.command,
        crate::commands::host::HostCommand::Manifest(crate::commands::host::ManifestArgs {
            json: true
        })
    ));
}

#[test]
fn cli_parses_emit_reflection_flag() {
    let cli = Cli::try_parse_from([
        "faber",
        "emit",
        "--reflection",
        "-t",
        "wgsl-text",
        "main.fab",
    ])
    .expect("parse emit");
    let Some(crate::cli::Command::Emit(args)) = cli.command else {
        panic!("expected emit subcommand");
    };
    assert!(args.reflection);
    assert_eq!(args.target, FaberCliTarget::WgslText);
}
