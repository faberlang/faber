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
fn cli_build_help_preserves_single_input_usage() {
    let mut command = Cli::command();
    let build = command
        .find_subcommand_mut("build")
        .expect("build subcommand");
    let help = build.render_long_help().to_string();

    assert!(help.contains("Usage: faber build [OPTIONS] <INPUT>"));
    assert!(!help.contains("Usage: faber build [OPTIONS] <INPUT>..."));
}

#[test]
fn cli_install_help_names_cista_store_only() {
    let mut command = Cli::command();
    let install = command
        .find_subcommand_mut("install")
        .expect("install subcommand");
    let help = install.render_long_help().to_string();

    assert!(help.contains("Cista package store"));
    assert!(help.contains("requires cista.toml"));
    assert!(!help.contains("FABER_LIBRARY_HOME"));
    assert!(!help.contains("--legacy-library-home"));
    assert!(help.contains("--path <PATH>"));
}

#[test]
fn cli_rejects_legacy_library_home_install_flag() {
    let err = Cli::try_parse_from(["faber", "install", "--legacy-library-home", "norma"])
        .expect_err("legacy install flag must be removed");
    assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
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
        "--include",
        "math*",
        "--exclude",
        "*edge*",
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
    assert_eq!(args.include, vec!["math*".to_owned()]);
    assert_eq!(args.exclude, vec!["*edge*".to_owned()]);
}

#[test]
fn cli_parses_test_filter_long_flag() {
    let cli = Cli::try_parse_from(["faber", "test", ".", "--filter", "smoke"]).expect("parse");
    let Some(crate::cli::Command::Test(args)) = cli.command else {
        panic!("expected test");
    };
    assert_eq!(args.filter_flag.as_deref(), Some("smoke"));
    assert!(args.filter.is_none());
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
    assert_eq!(args.target, FaberCliTarget::MirWgsl);
    assert_eq!(args.input, vec!["main.fab"]);
}

#[test]
fn cli_parses_emit_external_target_names() {
    for (name, expected) in [
        ("rust", FaberCliTarget::HirRust),
        ("faber", FaberCliTarget::HirFaber),
        ("ts", FaberCliTarget::HirTypeScript),
        ("go", FaberCliTarget::HirGo),
        ("wasm-text", FaberCliTarget::MirWasm),
        ("wasm", FaberCliTarget::MirWasmBinary),
        ("llvm-text", FaberCliTarget::MirLlvm),
        ("metal-text", FaberCliTarget::MirMetal),
        ("wgsl-text", FaberCliTarget::MirWgsl),
        ("sexp", FaberCliTarget::MirSexp),
    ] {
        let cli = Cli::try_parse_from(["faber", "emit", "-t", name, "main.fab"])
            .unwrap_or_else(|err| panic!("parse emit target {name}: {err}"));
        let Some(crate::cli::Command::Emit(args)) = cli.command else {
            panic!("expected emit subcommand for target {name}");
        };
        assert_eq!(args.target, expected, "target {name}");
    }
}

#[test]
fn cli_parses_lex_subcommand() {
    let lex = Cli::try_parse_from(["faber", "lex", "main.fab"]).expect("parse lex");
    let Some(crate::cli::Command::Lex(args)) = lex.command else {
        panic!("expected lex subcommand");
    };
    assert_eq!(args.input, vec!["main.fab"]);
}

#[test]
fn cli_parses_parse_subcommand() {
    let parse = Cli::try_parse_from(["faber", "parse", "main.fab"]).expect("parse parse");
    let Some(crate::cli::Command::Parse(args)) = parse.command else {
        panic!("expected parse subcommand");
    };
    assert_eq!(args.input, vec!["main.fab"]);
}

#[test]
fn cli_parses_hir_subcommand() {
    let hir = Cli::try_parse_from(["faber", "hir", "main.fab"]).expect("parse hir");
    let Some(crate::cli::Command::Hir(args)) = hir.command else {
        panic!("expected hir subcommand");
    };
    assert_eq!(args.input, vec!["main.fab"]);
}

#[test]
fn cli_parses_mir_subcommand() {
    let mir = Cli::try_parse_from(["faber", "mir", "main.fab"]).expect("parse mir");
    let Some(crate::cli::Command::Mir(args)) = mir.command else {
        panic!("expected mir subcommand");
    };
    assert_eq!(args.input, vec!["main.fab"]);
}

#[test]
fn cli_parses_cli_ir_subcommand() {
    let cli_ir = Cli::try_parse_from(["faber", "cli-ir", "main.fab"]).expect("parse cli-ir");
    let Some(crate::cli::Command::CliIr(args)) = cli_ir.command else {
        panic!("expected cli-ir subcommand");
    };
    assert_eq!(args.input, vec!["main.fab"]);
}

#[test]
fn cli_parses_scena_target_for_build() {
    let build = Cli::try_parse_from(["faber", "build", "--target", "scena", "pkg"])
        .expect("parse build scena target");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, Some(radix::tool::CliTarget::MirScena));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_scena_target_for_run() {
    let run = Cli::try_parse_from(["faber", "run", "--target", "scena", "pkg", "--", "Ian"])
        .expect("parse run scena target");
    let Some(crate::cli::Command::Run(args)) = run.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.target, Some(radix::tool::CliTarget::MirScena));
    assert_eq!(args.path, std::path::PathBuf::from("pkg"));
    assert_eq!(args.args, vec!["Ian".to_owned()]);
}

#[test]
fn cli_run_defaults_to_current_directory_and_implicit_target() {
    let run = Cli::try_parse_from(["faber", "run"]).expect("parse run defaults");
    let Some(crate::cli::Command::Run(args)) = run.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.path, std::path::PathBuf::from("."));
    // Unset target = implicit portable default (FHIR → FMIR), resolved from
    // the manifest at run time.
    assert_eq!(args.target, None);
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
    assert_eq!(args.target, Some(radix::tool::CliTarget::MirFmir));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_fmir_target_for_build() {
    let build = Cli::try_parse_from(["faber", "build", "--target", "fmir", "pkg"])
        .expect("parse build fmir target");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, Some(radix::tool::CliTarget::MirFmirBinary));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_fmir_bin_target_for_build() {
    let build = Cli::try_parse_from(["faber", "build", "--target", "fmir-bin", "pkg"])
        .expect("parse build fmir-bin target");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, Some(radix::tool::CliTarget::MirFmirBundle));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_fmir_bin_target_for_run() {
    let run = Cli::try_parse_from(["faber", "run", "--target", "fmir-bin", "pkg", "--", "Ian"])
        .expect("parse run fmir-bin target");
    let Some(crate::cli::Command::Run(args)) = run.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.target, Some(radix::tool::CliTarget::MirFmirBundle));
    assert_eq!(args.path, std::path::PathBuf::from("pkg"));
    assert_eq!(args.args, vec!["Ian".to_owned()]);
}

#[test]
fn cli_parses_locale_on_check() {
    let check = Cli::try_parse_from(["faber", "check", "--locale", "zh-Hans", "main.fab"])
        .expect("parse check reader locale");
    let Some(crate::cli::Command::Check(args)) = check.command else {
        panic!("expected check subcommand");
    };
    assert_eq!(args.locale.as_deref(), Some("zh-Hans"));
    assert_eq!(args.diagnostic_locale.as_deref(), None);
}

#[test]
fn cli_parses_diagnostic_locale_on_check() {
    let check = Cli::try_parse_from([
        "faber",
        "check",
        "--locale",
        "zh-Hans",
        "--diagnostic-locale",
        "th-TH",
        "main.fab",
    ])
    .expect("parse check diagnostic locale");
    let Some(crate::cli::Command::Check(args)) = check.command else {
        panic!("expected check subcommand");
    };
    assert_eq!(args.locale.as_deref(), Some("zh-Hans"));
    assert_eq!(args.diagnostic_locale.as_deref(), Some("th-TH"));
}

#[test]
fn cli_parses_diagnostic_locale_on_build_run_test_emit() {
    for (cmd, argv) in [
        (
            "build",
            vec!["faber", "build", "--diagnostic-locale", "en", "main.fab"],
        ),
        (
            "run",
            vec!["faber", "run", "--diagnostic-locale", "en", "main.fab"],
        ),
        (
            "test",
            vec!["faber", "test", "--diagnostic-locale", "en", "main.fab"],
        ),
        (
            "emit",
            vec![
                "faber",
                "emit",
                "--diagnostic-locale",
                "en",
                "-t",
                "rust",
                "main.fab",
            ],
        ),
    ] {
        let parsed = Cli::try_parse_from(argv).unwrap_or_else(|e| panic!("parse {cmd}: {e}"));
        match (cmd, parsed.command) {
            ("build", Some(crate::cli::Command::Build(args))) => {
                assert_eq!(args.diagnostic_locale.as_deref(), Some("en"));
            }
            ("run", Some(crate::cli::Command::Run(args))) => {
                assert_eq!(args.diagnostic_locale.as_deref(), Some("en"));
            }
            ("test", Some(crate::cli::Command::Test(args))) => {
                assert_eq!(args.diagnostic_locale.as_deref(), Some("en"));
            }
            ("emit", Some(crate::cli::Command::Emit(args))) => {
                assert_eq!(args.diagnostic_locale.as_deref(), Some("en"));
            }
            other => panic!("unexpected parse for {cmd}: {other:?}"),
        }
    }
}

#[test]
fn cli_parses_locale_on_emit() {
    let emit = Cli::try_parse_from([
        "faber", "emit", "--locale", "zh-Hans", "-t", "rust", "main.fab",
    ])
    .expect("parse emit reader locale");
    let Some(crate::cli::Command::Emit(args)) = emit.command else {
        panic!("expected emit subcommand");
    };
    assert_eq!(args.locale.as_deref(), Some("zh-Hans"));
}

#[test]
fn cli_parses_locale_on_build() {
    let build = Cli::try_parse_from(["faber", "build", "--locale", "zh-Hans", "main.fab"])
        .expect("parse build reader locale");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.locale.as_deref(), Some("zh-Hans"));
}

#[test]
fn cli_parses_locale_on_run() {
    let run = Cli::try_parse_from(["faber", "run", "--locale", "zh-Hans", "main.fab"])
        .expect("parse run reader locale");
    let Some(crate::cli::Command::Run(args)) = run.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.locale.as_deref(), Some("zh-Hans"));
}

#[test]
fn cli_parses_locale_on_test() {
    let test = Cli::try_parse_from(["faber", "test", "--locale", "zh-Hans", "main.fab"])
        .expect("parse test reader locale");
    let Some(crate::cli::Command::Test(args)) = test.command else {
        panic!("expected test subcommand");
    };
    assert_eq!(args.locale.as_deref(), Some("zh-Hans"));
}

#[test]
fn cli_parses_locale_on_format() {
    let format = Cli::try_parse_from(["faber", "format", "--locale", "zh-Hans", "main.fab"])
        .expect("parse format reader locale");
    let Some(crate::cli::Command::Format(args)) = format.command else {
        panic!("expected format subcommand");
    };
    assert_eq!(args.locale.as_deref(), Some("zh-Hans"));
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
fn cli_build_rejects_extra_direct_inputs() {
    let error = Cli::try_parse_from(["faber", "build", "main.fab", "other.fab"])
        .expect_err("build extra direct inputs");
    let rendered = error.to_string();
    assert!(rendered.contains("unexpected argument 'other.fab'"));
    assert!(rendered.contains("Usage: faber build [OPTIONS] <INPUT>"));
}

#[test]
fn cli_run_rejects_extra_direct_inputs_without_double_dash() {
    let error = Cli::try_parse_from(["faber", "run", "main.fab", "other.fab"])
        .expect_err("run extra direct inputs without --");
    let rendered = error.to_string();
    assert!(rendered.contains("unexpected argument 'other.fab'"));
    assert!(rendered.contains("Usage: faber run [OPTIONS] [PATH] [-- <ARGS>...]"));
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
fn cli_parses_diagnostic_locale_on_explain() {
    let explain = Cli::try_parse_from([
        "faber",
        "explain",
        "--diagnostic-locale",
        "zh-Hans",
        "SEM010.initializer_annotation_mismatch",
    ])
    .expect("parse explain diagnostic locale");
    let Some(crate::cli::Command::Explain(args)) = explain.command else {
        panic!("expected explain subcommand");
    };
    assert_eq!(args.diagnostic_locale.as_deref(), Some("zh-Hans"));
    assert_eq!(
        args.term.as_deref(),
        Some("SEM010.initializer_annotation_mismatch")
    );
}

#[test]
fn cli_parses_explain_json_term() {
    let json =
        Cli::try_parse_from(["faber", "explain", "--json", "nihil"]).expect("parse explain json");
    let Some(crate::cli::Command::Explain(json_args)) = json.command else {
        panic!("expected explain subcommand");
    };
    assert!(json_args.json);
    assert_eq!(json_args.term.as_deref(), Some("nihil"));
}

#[test]
fn cli_parses_explain_search() {
    let search = Cli::try_parse_from(["faber", "explain", "--search", "host"])
        .expect("parse explain search");
    let Some(crate::cli::Command::Explain(search_args)) = search.command else {
        panic!("expected explain subcommand");
    };
    assert_eq!(search_args.search.as_deref(), Some("host"));
    assert!(search_args.term.is_none());
}

#[test]
fn cli_parses_explain_list() {
    let list = Cli::try_parse_from(["faber", "explain", "--list"]).expect("parse explain list");
    let Some(crate::cli::Command::Explain(list_args)) = list.command else {
        panic!("expected explain subcommand");
    };
    assert!(list_args.list);
    assert!(list_args.term.is_none());
}

#[test]
fn cli_parses_explain_category() {
    let category = Cli::try_parse_from(["faber", "explain", "--category", "diagnostics"])
        .expect("parse explain category");
    let Some(crate::cli::Command::Explain(category_args)) = category.command else {
        panic!("expected explain subcommand");
    };
    assert_eq!(category_args.category.as_deref(), Some("diagnostics"));
    assert!(category_args.term.is_none());
}

#[test]
fn cli_rejects_explain_list_and_term() {
    let mixed = Cli::try_parse_from(["faber", "explain", "--list", "nihil"])
        .expect_err("list and term should conflict");
    let mixed_rendered = mixed.to_string();
    assert!(mixed_rendered.contains("--list"));
}

#[test]
fn cli_rejects_explain_search_and_json() {
    let search_json = Cli::try_parse_from(["faber", "explain", "--search", "host", "--json"])
        .expect_err("search and json should conflict");
    let search_json_rendered = search_json.to_string();
    assert!(search_json_rendered.contains("--search"));
    assert!(search_json_rendered.contains("--json"));
}

#[test]
fn cli_rejects_explain_search_and_category() {
    let search_category = Cli::try_parse_from([
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
}

#[test]
fn cli_rejects_explain_list_and_diagnostic_locale() {
    let list_locale =
        Cli::try_parse_from(["faber", "explain", "--list", "--diagnostic-locale", "la"])
            .expect_err("list and diagnostic locale should conflict");
    let list_locale_rendered = list_locale.to_string();
    assert!(list_locale_rendered.contains("--list"));
    assert!(list_locale_rendered.contains("--diagnostic-locale"));
}

#[test]
fn cli_rejects_explain_search_and_diagnostic_locale() {
    let search_locale = Cli::try_parse_from([
        "faber",
        "explain",
        "--search",
        "host",
        "--diagnostic-locale",
        "la",
    ])
    .expect_err("search and diagnostic locale should conflict");
    let search_locale_rendered = search_locale.to_string();
    assert!(search_locale_rendered.contains("--search"));
    assert!(search_locale_rendered.contains("--diagnostic-locale"));
}

#[test]
fn cli_rejects_explain_category_and_diagnostic_locale() {
    let category_locale = Cli::try_parse_from([
        "faber",
        "explain",
        "--category",
        "diagnostics",
        "--diagnostic-locale",
        "la",
    ])
    .expect_err("category and diagnostic locale should conflict");
    let category_locale_rendered = category_locale.to_string();
    assert!(category_locale_rendered.contains("--category"));
    assert!(category_locale_rendered.contains("--diagnostic-locale"));
}

#[test]
fn cli_rejects_explain_json_without_term() {
    let error =
        Cli::try_parse_from(["faber", "explain", "--json"]).expect_err("json requires a term");
    let rendered = error.to_string();
    assert!(rendered.contains("--json"));
    assert!(rendered.contains("<TERM>") || rendered.contains("<term>"));
}

#[test]
fn cli_rejects_explain_diagnostic_locale_without_term() {
    let error = Cli::try_parse_from(["faber", "explain", "--diagnostic-locale", "la"])
        .expect_err("diagnostic locale requires a term");
    let rendered = error.to_string();
    assert!(rendered.contains("required arguments were not provided"));
    assert!(rendered.contains("<TERM>") || rendered.contains("<term>"));
}

#[test]
fn cli_parses_explain_diagnostic_locale_with_term() {
    let cli = Cli::try_parse_from(["faber", "explain", "--diagnostic-locale", "th-TH", "SEM010"])
        .expect("parse explain diagnostic locale");
    let Some(crate::cli::Command::Explain(args)) = cli.command else {
        panic!("expected explain subcommand");
    };
    assert_eq!(args.diagnostic_locale.as_deref(), Some("th-TH"));
    assert_eq!(args.term.as_deref(), Some("SEM010"));
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
    assert_eq!(args.library.as_deref(), Some("norma"));
    assert!(args.path.is_none());
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
fn cli_script_defaults_to_current_directory() {
    let script = Cli::try_parse_from(["faber", "script"]).expect("parse script defaults");
    let Some(crate::cli::Command::Script(script_args)) = script.command else {
        panic!("expected script subcommand");
    };
    assert_eq!(script_args.path, std::path::PathBuf::from("."));
    assert!(script_args.args.is_empty());
}

#[test]
fn cli_test_defaults_to_current_directory() {
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
    assert_eq!(args.target, FaberCliTarget::MirWgsl);
}

#[test]
fn cli_rejects_unknown_subcommand() {
    let err = Cli::try_parse_from(["faber", "unknown-command"])
        .expect_err("unknown subcommand must be rejected");
    assert!(matches!(
        err.kind(),
        clap::error::ErrorKind::InvalidSubcommand | clap::error::ErrorKind::UnknownArgument
    ));
}

#[test]
fn cli_rejects_build_without_input() {
    let err = Cli::try_parse_from(["faber", "build"]).expect_err("build requires an input");
    assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
}

#[test]
fn cli_defaults_emit_target_to_rust() {
    let cli =
        Cli::try_parse_from(["faber", "emit", "main.fab"]).expect("parse emit default target");
    let Some(crate::cli::Command::Emit(args)) = cli.command else {
        panic!("expected emit subcommand");
    };
    assert_eq!(args.target, FaberCliTarget::HirRust);
}
