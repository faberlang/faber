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
fn cli_rejects_install_subcommand() {
    // S2-U2 (faber-radix-recomposition): `faber install` is removed. Cista owns
    // package acquisition (`cista install`); the product CLI rejects the
    // subcommand as unknown, so no store/temp-checkout side effect is reachable.
    for argv in [
        &["faber", "install"][..],
        &["faber", "install", "norma"][..],
        &["faber", "install", "--path", "."][..],
    ] {
        let err = Cli::try_parse_from(argv).expect_err("install subcommand is removed");
        // clap reports an unexpected trailing argument (`UnknownArgument`) for
        // a non-existent subcommand under `args_conflicts_with_subcommands`
        // with an optional subcommand enum; either kind proves rejection.
        assert!(
            matches!(
                err.kind(),
                clap::error::ErrorKind::InvalidSubcommand | clap::error::ErrorKind::UnknownArgument
            ),
            "argv {argv:?} must reject `faber install` as an unknown subcommand"
        );
    }
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
fn targets_table_reports_capability_truth_for_the_default_build() {
    // The cli_test binary builds under default features (full-targets): the
    // rust host lane must be present, and the opt-in capability rows absent —
    // `device-runtime` / `host-macos-arm64` left full-targets at the S3-U2
    // ordinary-tool cut (f683d94d0), so capability truth reports no row for
    // an unbuilt surface.
    let table = crate::commands::targets::rendered_targets_table();
    assert!(
        table.contains("rust available=yes"),
        "rust host lane must be reported as available:\n{table}"
    );
    for row_name in ["device-runtime", "host-macos-arm64"] {
        assert!(
            !table
                .lines()
                .any(|line| line.starts_with(&format!("{row_name} "))),
            "unexpected {row_name} capability row under the default build (feature left full-targets):\n{table}"
        );
    }
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
    assert_eq!(args.suite, vec!["suite/path".to_owned()]);
    assert_eq!(args.tag, vec!["slow".to_owned()]);
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
    assert_eq!(args.filter_flag, vec!["smoke".to_owned()]);
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
        ("python", FaberCliTarget::HirPython),
        ("haskell", FaberCliTarget::HirHaskell),
        ("wasm-text", FaberCliTarget::MirWasm),
        ("wasm", FaberCliTarget::MirWasm),
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
    assert_eq!(args.target, Some(FaberCliTarget::MirScena));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_run_defaults_to_current_directory() {
    let run = Cli::try_parse_from(["faber", "run"]).expect("parse run defaults");
    let Some(crate::cli::Command::Run(args)) = run.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.path, std::path::PathBuf::from("."));
    assert!(!args.deny_warnings);
    assert!(args.args.is_empty());
}

#[test]
fn cli_rejects_removed_run_mode_flags() {
    // `faber run` is interpreted execution, always; the `--interpret` /
    // `--compile` mode-switch flags retired with the Stage 6 clean break and
    // must fail at parse time, never silently accept.
    for flag in ["--interpret", "--compile"] {
        let error =
            Cli::try_parse_from(["faber", "run", flag, "pkg"]).expect_err("removed mode flag");
        let rendered = error.to_string();
        assert!(
            rendered.contains(flag),
            "removed run mode flag {flag} must be rejected:\n{rendered}"
        );
    }
}

#[test]
fn cli_rejects_removed_run_target_flag() {
    // `run` no longer selects a compiled target; targets are built with
    // `faber build -t <target>`. The `-t` flag on run must fail at parse.
    for argv in [
        &["faber", "run", "--target", "scena", "pkg"][..],
        &["faber", "run", "--target", "fmir-bin", "pkg"][..],
    ] {
        let error = Cli::try_parse_from(argv).expect_err("removed run target flag");
        let rendered = error.to_string();
        assert!(
            rendered.contains("--target"),
            "removed run --target must be rejected:\n{rendered}"
        );
    }
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
    assert_eq!(args.target, Some(FaberCliTarget::MirFmir));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_fmir_target_for_build() {
    let build = Cli::try_parse_from(["faber", "build", "--target", "fmir", "pkg"])
        .expect("parse build fmir target");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, Some(FaberCliTarget::MirFmirBinary));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_fmir_bin_target_for_build() {
    let build = Cli::try_parse_from(["faber", "build", "--target", "fmir-bin", "pkg"])
        .expect("parse build fmir-bin target");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, Some(FaberCliTarget::MirFmirBundle));
    assert_eq!(args.input, "pkg");
}

#[test]
fn cli_parses_locale_on_check() {
    let check = Cli::try_parse_from(["faber", "check", "--locale", "zh-Hans", "main.fab"])
        .expect("parse check reader locale");
    let Some(crate::cli::Command::Check(args)) = check.command else {
        panic!("expected check subcommand");
    };
    assert_eq!(args.locale.as_deref(), Some("zh-Hans"));
    assert_eq!(args.diagnostics_locale.as_deref(), None);
}

#[test]
fn cli_parses_diagnostics_locale_on_check() {
    let check = Cli::try_parse_from([
        "faber",
        "check",
        "--locale",
        "zh-Hans",
        "--diagnostics-locale",
        "th-TH",
        "main.fab",
    ])
    .expect("parse check diagnostic locale");
    let Some(crate::cli::Command::Check(args)) = check.command else {
        panic!("expected check subcommand");
    };
    assert_eq!(args.locale.as_deref(), Some("zh-Hans"));
    assert_eq!(args.diagnostics_locale.as_deref(), Some("th-TH"));
}

#[test]
fn cli_parses_diagnostics_locale_on_build_run_test_emit() {
    for (cmd, argv) in [
        (
            "build",
            vec!["faber", "build", "--diagnostics-locale", "en", "main.fab"],
        ),
        (
            "run",
            vec!["faber", "run", "--diagnostics-locale", "en", "main.fab"],
        ),
        (
            "test",
            vec!["faber", "test", "--diagnostics-locale", "en", "main.fab"],
        ),
        (
            "emit",
            vec![
                "faber",
                "emit",
                "--diagnostics-locale",
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
                assert_eq!(args.diagnostics_locale.as_deref(), Some("en"));
            }
            ("run", Some(crate::cli::Command::Run(args))) => {
                assert_eq!(args.diagnostics_locale.as_deref(), Some("en"));
            }
            ("test", Some(crate::cli::Command::Test(args))) => {
                assert_eq!(args.diagnostics_locale.as_deref(), Some("en"));
            }
            ("emit", Some(crate::cli::Command::Emit(args))) => {
                assert_eq!(args.diagnostics_locale.as_deref(), Some("en"));
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
fn cli_parses_convert_target_locale() {
    let convert = Cli::try_parse_from(["faber", "convert", "--to", "zh-Hans", "main.fab"])
        .expect("parse convert target locale");
    let Some(crate::cli::Command::Convert(args)) = convert.command else {
        panic!("expected convert subcommand");
    };
    assert_eq!(args.to, "zh-Hans");
}

#[test]
fn cli_rejects_removed_format_locale_flag() {
    let result = Cli::try_parse_from(["faber", "format", "--locale", "zh-Hans", "main.fab"]);
    assert!(
        result.is_err(),
        "format --locale is retired; locale conversion is `faber convert --to`"
    );
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
fn cli_parses_format_write_flag() {
    let format = Cli::try_parse_from(["faber", "format", "--write", "main.fab"])
        .expect("parse format --write");
    let Some(crate::cli::Command::Format(args)) = format.command else {
        panic!("expected format subcommand");
    };
    assert!(args.write);
    assert!(!args.stdout);
    assert_eq!(args.paths, vec![std::path::PathBuf::from("main.fab")]);
}

#[test]
fn cli_rejects_format_write_with_stdout() {
    let error = Cli::try_parse_from(["faber", "format", "--write", "--stdout", "main.fab"])
        .expect_err("format --write conflicts with --stdout");
    let rendered = error.to_string();
    assert!(rendered.contains("--write"));
    assert!(rendered.contains("--stdout"));
}

#[test]
fn cli_parses_format_stdin_flag() {
    let format = Cli::try_parse_from(["faber", "format", "--stdin"]).expect("parse format --stdin");
    let Some(crate::cli::Command::Format(args)) = format.command else {
        panic!("expected format subcommand");
    };
    assert!(args.stdin);
    assert!(args.paths.is_empty());
}

#[test]
fn cli_rejects_format_stdin_with_paths() {
    let error = Cli::try_parse_from(["faber", "format", "--stdin", "main.fab"])
        .expect_err("format --stdin takes no paths");
    let rendered = error.to_string();
    assert!(rendered.contains("--stdin"));
    assert!(
        rendered.contains("[PATH]"),
        "the conflict must name the path argument: {rendered}"
    );
}

#[test]
fn cli_rejects_format_stdin_with_check() {
    let error = Cli::try_parse_from(["faber", "format", "--stdin", "--check"])
        .expect_err("format --stdin conflicts with --check");
    let rendered = error.to_string();
    assert!(rendered.contains("--stdin"));
    assert!(rendered.contains("--check"));
}

#[test]
fn cli_rejects_format_policy_flag() {
    let error = Cli::try_parse_from(["faber", "format", "--policy", "pretty-v1", "main.fab"])
        .expect_err("format --policy is retired");
    let rendered = error.to_string();
    assert!(
        rendered.contains("--policy"),
        "retired --policy must fail at parse: {rendered}"
    );
}

#[test]
fn cli_rejects_format_config_flag() {
    let error = Cli::try_parse_from(["faber", "format", "--config", "forma.toml", "main.fab"])
        .expect_err("format --config is retired");
    let rendered = error.to_string();
    assert!(
        rendered.contains("--config"),
        "retired --config must fail at parse: {rendered}"
    );
}

#[test]
fn cli_parses_diagnostics_locale_on_explain() {
    let explain = Cli::try_parse_from([
        "faber",
        "explain",
        "--diagnostics-locale",
        "zh-Hans",
        "SEM010.initializer_annotation_mismatch",
    ])
    .expect("parse explain diagnostic locale");
    let Some(crate::cli::Command::Explain(args)) = explain.command else {
        panic!("expected explain subcommand");
    };
    assert_eq!(args.diagnostics_locale.as_deref(), Some("zh-Hans"));
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
fn cli_rejects_explain_list_and_diagnostics_locale() {
    let list_locale =
        Cli::try_parse_from(["faber", "explain", "--list", "--diagnostics-locale", "la"])
            .expect_err("list and diagnostic locale should conflict");
    let list_locale_rendered = list_locale.to_string();
    assert!(list_locale_rendered.contains("--list"));
    assert!(list_locale_rendered.contains("--diagnostics-locale"));
}

#[test]
fn cli_rejects_explain_search_and_diagnostics_locale() {
    let search_locale = Cli::try_parse_from([
        "faber",
        "explain",
        "--search",
        "host",
        "--diagnostics-locale",
        "la",
    ])
    .expect_err("search and diagnostic locale should conflict");
    let search_locale_rendered = search_locale.to_string();
    assert!(search_locale_rendered.contains("--search"));
    assert!(search_locale_rendered.contains("--diagnostics-locale"));
}

#[test]
fn cli_rejects_explain_category_and_diagnostics_locale() {
    let category_locale = Cli::try_parse_from([
        "faber",
        "explain",
        "--category",
        "diagnostics",
        "--diagnostics-locale",
        "la",
    ])
    .expect_err("category and diagnostic locale should conflict");
    let category_locale_rendered = category_locale.to_string();
    assert!(category_locale_rendered.contains("--category"));
    assert!(category_locale_rendered.contains("--diagnostics-locale"));
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
fn cli_rejects_explain_diagnostics_locale_without_term() {
    let error = Cli::try_parse_from(["faber", "explain", "--diagnostics-locale", "la"])
        .expect_err("diagnostic locale requires a term");
    let rendered = error.to_string();
    assert!(rendered.contains("required arguments were not provided"));
    assert!(rendered.contains("<TERM>") || rendered.contains("<term>"));
}

#[test]
fn cli_parses_explain_diagnostics_locale_with_term() {
    let cli = Cli::try_parse_from([
        "faber",
        "explain",
        "--diagnostics-locale",
        "th-TH",
        "SEM010",
    ])
    .expect("parse explain diagnostic locale");
    let Some(crate::cli::Command::Explain(args)) = cli.command else {
        panic!("expected explain subcommand");
    };
    assert_eq!(args.diagnostics_locale.as_deref(), Some("th-TH"));
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
fn cli_init_defaults_to_current_directory() {
    let cli = Cli::try_parse_from(["faber", "init"]).expect("parse init");
    let Some(crate::cli::Command::Init(args)) = cli.command else {
        panic!("expected init subcommand");
    };
    assert_eq!(args.path, std::path::PathBuf::from("."));
}

#[test]
fn init_scaffold_is_en_surface_so_fresh_package_checks() {
    // Regression (2026-08-15): the scaffold used latin keywords while
    // untagged sources parse on the en surface, so `faber init` produced a
    // package that failed its own `faber check`.
    let dir = std::env::temp_dir().join(format!(
        "faber-init-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp root");
    let project = dir.join("demo");
    crate::commands::init::scaffold(&project, "demo").expect("scaffold");
    let entry = std::fs::read_to_string(project.join("src/main.fab")).expect("read scaffold");
    assert!(
        entry.starts_with("main {") && entry.contains("print \"Salve, munde!\""),
        "scaffold must be en surface, got: {entry}"
    );
    std::fs::remove_dir_all(&dir).ok();
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
fn verify_library_long_help_states_faber_toml_requirement() {
    let mut command = Cli::command();
    let verify_library = command
        .find_subcommand_mut("verify-library")
        .expect("verify-library subcommand");
    let help = verify_library.render_long_help().to_string();

    assert!(
        help.contains("Requires a faber.toml manifest at the library root"),
        "verify-library --help must state the faber.toml requirement:\n{help}"
    );
}

#[test]
fn verify_library_missing_manifest_names_sought_faber_toml_path() {
    let dir = std::env::temp_dir().join(format!(
        "faber-verify-library-missing-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create manifest-less dir");
    let sought = dir.join("faber.toml");
    let diagnostics = crate::package::verify_library_bindings(&dir, "rust")
        .expect_err("manifest-less dir must fail at faber.toml");
    std::fs::remove_dir_all(&dir).ok();

    let names_sought = diagnostics.iter().any(|diagnostic| {
        diagnostic.file == sought.display().to_string()
            || diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "path" && arg.value == sought.display().to_string())
    });
    assert!(
        names_sought,
        "verify-library on a manifest-less dir must name the sought faber.toml path: {diagnostics:?}"
    );
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

#[test]
fn cli_parses_doctor_subcommand() {
    let cli = Cli::try_parse_from(["faber", "doctor"]).expect("parse doctor");
    assert!(matches!(cli.command, Some(crate::cli::Command::Doctor)));
}

#[test]
fn cli_parses_check_library_subcommand() {
    let cli =
        Cli::try_parse_from(["faber", "check-library", "norma"]).expect("parse check-library");
    let Some(crate::cli::Command::CheckLibrary(args)) = cli.command else {
        panic!("expected check-library subcommand");
    };
    assert_eq!(args.name, "norma");
}

#[test]
fn check_long_help_discloses_library_resolution() {
    let mut command = Cli::command();
    let check = command
        .find_subcommand_mut("check")
        .expect("check subcommand");
    let help = check.render_long_help().to_string();

    assert!(
        help.contains("FABER_LIBRARY_HOME"),
        "check --help must name FABER_LIBRARY_HOME:\n{help}"
    );
    assert!(
        help.contains("ancestor probing from the input path"),
        "check --help must name ancestor probing from the input path:\n{help}"
    );
    assert!(
        help.contains("faber.toml"),
        "check --help must name the manifest requirement:\n{help}"
    );
    assert!(
        help.contains("locale = \"la\""),
        "check --help must document the locale = \"la\" pin trap:\n{help}"
    );
    assert!(
        help.contains("builtin"),
        "check --help must name the builtin-vs-package taxonomy:\n{help}"
    );
}

#[test]
fn check_long_help_states_check_ownership_split() {
    let mut command = Cli::command();
    let check = command
        .find_subcommand_mut("check")
        .expect("check subcommand");
    let help = check.render_long_help().to_string();
    let help_line = help
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    assert!(
        help_line.contains("single-file compiler-dev checks"),
        "faber check --help must state radix owns single-file compiler-dev checks:\n{help}"
    );
    assert!(
        help_line.contains("package checking, including auto-detected packages"),
        "faber check --help must state faber owns package checking, including auto-detected packages:\n{help}"
    );
}

#[test]
fn check_file_package_refusal_matches_radix_rerun_instruction() {
    assert_eq!(
        crate::commands::PACKAGE_CHECK_MESSAGE,
        "error: package checking is owned by the `faber` tool; rerun with `faber check --package`"
    );
}

#[test]
fn check_library_long_help_states_v1_library_home_rule() {
    let mut command = Cli::command();
    let check_library = command
        .find_subcommand_mut("check-library")
        .expect("check-library subcommand");
    let help = check_library.render_long_help().to_string();

    assert!(help.contains("library home"));
    assert!(help.contains("does not parse"));
    assert!(help.contains("cista.toml"));
    assert!(help.contains("scripta/check-source"));
}
