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

#[cfg(all(feature = "full-targets", feature = "hir-rust"))]
#[test]
fn targets_table_reports_capability_truth_for_the_default_build() {
    // The cli_test binary builds under default features (full-targets): the
    // rust host lane must be present, and so must the device-runtime
    // capability row — `device-runtime` is part of full-targets again (A2
    // closeout), so a stock `cargo build -p faber` can dispatch to Metal.
    let table = crate::commands::targets::rendered_targets_table()
        .expect("every CLI target maps to a faber feature");
    assert!(
        table.contains("rust available=yes"),
        "rust host lane must be reported as available:\n{table}"
    );
    {
        let row_name = "device-runtime";
        assert!(
            table
                .lines()
                .any(|line| line.starts_with(&format!("{row_name} "))),
            "missing {row_name} capability row under the default build:\n{table}"
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
        ("basic", FaberCliTarget::MirBasic),
        ("bas", FaberCliTarget::MirBasic),
        ("pascal", FaberCliTarget::HirPascal),
        ("pas", FaberCliTarget::HirPascal),
        ("cobol", FaberCliTarget::HirCobol),
        ("cob", FaberCliTarget::HirCobol),
        ("fortran", FaberCliTarget::HirFortran),
        ("f90", FaberCliTarget::HirFortran),
        ("odin", FaberCliTarget::HirOdin),
        ("c", FaberCliTarget::HirC),
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
    assert!(args.threads.is_none());
    assert!(!args.deny_warnings);
    assert!(!args.stats);
    assert!(args.stats_file.is_none());
    assert!(args.args.is_empty());
}

#[test]
fn cli_parses_run_stats_sinks_and_rejects_profile_name() {
    let stats = Cli::try_parse_from(["faber", "run", "--stats", "pkg"]).expect("parse --stats");
    let Some(crate::cli::Command::Run(args)) = stats.command else {
        panic!("expected run command");
    };
    assert!(args.stats);
    assert!(args.stats_file.is_none());
    assert_eq!(args.path, std::path::PathBuf::from("pkg"));

    let stats_file = Cli::try_parse_from(["faber", "run", "--stats-file", "out.json", "pkg"])
        .expect("parse --stats-file");
    let Some(crate::cli::Command::Run(args)) = stats_file.command else {
        panic!("expected run command");
    };
    assert!(!args.stats);
    assert_eq!(args.stats_file, Some(std::path::PathBuf::from("out.json")));

    let both = Cli::try_parse_from(["faber", "run", "--stats", "--stats-file", "out.json", "pkg"])
        .expect("parse both stats sinks");
    let Some(crate::cli::Command::Run(args)) = both.command else {
        panic!("expected run command");
    };
    assert!(args.stats);
    assert_eq!(args.stats_file, Some(std::path::PathBuf::from("out.json")));

    let error = Cli::try_parse_from(["faber", "run", "--profile", "debug", "pkg"])
        .expect_err("--profile is not a run stats spelling");
    assert!(error.to_string().contains("--profile"));
}

#[test]
fn run_accepts_shuffle_seed_and_debug_flags() {
    let cli = Cli::try_parse_from(["faber", "run", "--shuffle-seed", "0", "--debug", "x.fab"])
        .expect("parse run shuffle-seed and debug");
    let Some(crate::cli::Command::Run(args)) = cli.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(args.shuffle_seed, Some(0));
    assert!(args.debug);
}

#[test]
fn test_accepts_only_in_flag() {
    let cli =
        Cli::try_parse_from(["faber", "test", "--only-in", "ci"]).expect("parse test --only-in");
    let Some(crate::cli::Command::Test(args)) = cli.command else {
        panic!("expected test subcommand");
    };
    assert_eq!(args.only_in.as_deref(), Some("ci"));
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
fn cli_parses_complexity_budget_on_check() {
    let check = Cli::try_parse_from(["faber", "check", "--complexity-budget", "7", "main.fab"])
        .expect("parse check complexity budget");
    let Some(crate::cli::Command::Check(args)) = check.command else {
        panic!("expected check subcommand");
    };
    assert_eq!(args.complexity_budget, Some(7));
}

#[test]
fn cli_complexity_budget_absent_defaults_to_none_on_check() {
    let check = Cli::try_parse_from(["faber", "check", "main.fab"]).expect("parse check");
    let Some(crate::cli::Command::Check(args)) = check.command else {
        panic!("expected check subcommand");
    };
    assert_eq!(args.complexity_budget, None);
}

#[test]
fn cli_complexity_budget_rejects_zero_and_non_numeric() {
    let zero = Cli::try_parse_from(["faber", "check", "--complexity-budget", "0", "main.fab"]);
    assert!(zero.is_err(), "budget 0 must fail clap validation (>= 1)");
    let text = Cli::try_parse_from(["faber", "check", "--complexity-budget", "abc", "main.fab"]);
    assert!(
        text.is_err(),
        "non-numeric budget must fail clap u32 parsing"
    );
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
fn cli_convert_and_field_modifier_codemod_parse_write_flag() {
    let convert = Cli::try_parse_from(["faber", "convert", "--to", "en", "--write", "main.fab"])
        .expect("parse convert --write");
    let Some(crate::cli::Command::Convert(args)) = convert.command else {
        panic!("expected convert subcommand");
    };
    assert!(args.write && !args.check && !args.stdout);

    let codemod = Cli::try_parse_from(["faber", "field-modifier-codemod", "--write", "main.fab"])
        .expect("parse field-modifier-codemod --write");
    let Some(crate::cli::Command::FieldModifierCodemod(args)) = codemod.command else {
        panic!("expected field-modifier-codemod subcommand");
    };
    assert!(args.write && !args.check && !args.stdout);
}

#[test]
fn convert_and_field_modifier_codemod_help_state_report_default_and_write_flag() {
    let mut command = Cli::command();
    let convert = command
        .find_subcommand_mut("convert")
        .expect("convert subcommand")
        .render_long_help()
        .to_string();
    let words = convert.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        words.contains("Does not write unless `--write` is given"),
        "{convert}"
    );
    assert!(
        words.contains("Write converted source back to the file (default is report-only)"),
        "{convert}"
    );

    let mut command = Cli::command();
    let codemod = command
        .find_subcommand_mut("field-modifier-codemod")
        .expect("field-modifier-codemod subcommand")
        .render_long_help()
        .to_string();
    let words = codemod.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        words.contains("Write codemodded source back to the file (default is report-only)"),
        "{codemod}"
    );
}

#[test]
fn locale_command_help_states_complete_unit_and_resolution_contract() {
    let mut command = Cli::command();
    let format = command
        .find_subcommand_mut("format")
        .expect("format subcommand")
        .render_long_help()
        .to_string();
    let format_words = format.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        format_words.contains("Format complete Faber source units"),
        "{format}"
    );
    assert!(
        format_words.contains("frontmatter, then the package manifest"),
        "{format}"
    );
    assert!(format_words.contains("complete function or"), "{format}");
    assert!(format_words.contains("`main` unit"), "{format}");

    let mut command = Cli::command();
    let convert = command
        .find_subcommand_mut("convert")
        .expect("convert subcommand")
        .render_long_help()
        .to_string();
    let convert_words = convert.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        convert_words.contains("Convert complete Faber source units"),
        "{convert}"
    );
    assert!(convert_words.contains("--from"), "{convert}");
    assert!(
        convert_words.contains("one command-wide target pack"),
        "{convert}"
    );
    assert!(
        convert_words.contains("preserving unrelated metadata"),
        "{convert}"
    );
    assert!(convert_words.contains("wrapper such as"), "{convert}");
    assert!(convert_words.contains("main {"), "{convert}");
}

#[test]
fn cli_parses_check_flags_and_multiple_inputs() {
    let check = Cli::try_parse_from([
        "faber",
        "check",
        "--no-diagnostics",
        "--permissive",
        "--package",
        "main.fab",
        "other.fab",
    ])
    .expect("parse check flags");
    let Some(crate::cli::Command::Check(args)) = check.command else {
        panic!("expected check subcommand");
    };
    assert!(args.no_diagnostics);
    assert!(args.permissive);
    assert!(args.package);
    assert_eq!(args.input, vec!["main.fab", "other.fab"]);

    assert!(
        Cli::try_parse_from(["faber", "check", "--diagnostics"]).is_err(),
        "--diagnostics is removed rather than kept as an alias"
    );
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
fn cli_format_default_mode_is_report_only() {
    let format = Cli::try_parse_from(["faber", "format", "main.fab"]).expect("parse format");
    let Some(crate::cli::Command::Format(args)) = format.command else {
        panic!("expected format subcommand");
    };
    assert!(!args.write);
    assert!(!args.check);
    assert!(!args.stdout);
    assert!(!args.stdin);
}

#[test]
fn cli_parses_format_check_flag() {
    let format = Cli::try_parse_from(["faber", "format", "--check"]).expect("parse format --check");
    let Some(crate::cli::Command::Format(args)) = format.command else {
        panic!("expected format subcommand");
    };
    assert!(args.check);
    assert!(!args.write);
}

#[test]
fn cli_rejects_format_write_with_check() {
    let error = Cli::try_parse_from(["faber", "format", "--write", "--check", "main.fab"])
        .expect_err("format --write conflicts with --check");
    let rendered = error.to_string();
    assert!(rendered.contains("--write"));
    assert!(rendered.contains("--check"));
}

#[test]
fn cli_rejects_format_stdin_with_write_or_stdout() {
    for other in ["--write", "--stdout"] {
        let error = Cli::try_parse_from(["faber", "format", "--stdin", other])
            .expect_err("format --stdin conflicts with the other modes");
        let rendered = error.to_string();
        assert!(rendered.contains("--stdin"), "{rendered}");
        assert!(rendered.contains(other), "{rendered}");
    }
}

#[test]
fn format_help_states_report_default_and_write_flag() {
    let mut command = Cli::command();
    let format = command
        .find_subcommand_mut("format")
        .expect("format subcommand")
        .render_long_help()
        .to_string();
    let words = format.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        words.contains("Does not write unless `--write` is given"),
        "{format}"
    );
    assert!(
        words.contains("Write formatted source back to the file (default is report-only)"),
        "{format}"
    );
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
fn cli_parses_explain_list_json() {
    let list = Cli::try_parse_from(["faber", "explain", "--list", "--json"])
        .expect("parse explain list json");
    let Some(crate::cli::Command::Explain(list_args)) = list.command else {
        panic!("expected explain subcommand");
    };
    assert!(list_args.list);
    assert!(list_args.json);
}

#[test]
fn cli_rejects_explain_json_without_term_or_list() {
    assert!(Cli::try_parse_from(["faber", "explain", "--json"]).is_err());
    assert!(Cli::try_parse_from(["faber", "explain", "--json", "--search", "host"]).is_err());
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
    // NEED-5 (f4f9ca943): usage is now `<TERM|--list>`; `<TERM` is the stable prefix.
    assert!(rendered.contains("<TERM") || rendered.contains("<term"));
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
        entry.starts_with("main {") && entry.contains("print \"Hello, world!\""),
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
    let diagnostics = faber_cli::package::verify_library_bindings(&dir, "rust")
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
    assert!(matches!(cli.command, Some(crate::cli::Command::Doctor(_))));
}

#[test]
fn check_accepts_library_home_flag() {
    let cli = Cli::try_parse_from(["faber", "check", "--library-home", "/x", "main.fab"])
        .expect("parse check --library-home");
    let Some(crate::cli::Command::Check(args)) = cli.command else {
        panic!("expected check subcommand");
    };
    assert_eq!(
        args.library.library_home.as_deref(),
        Some(std::path::Path::new("/x"))
    );
    let (flag, probe) = crate::cli::Command::Check(args).library_home_inputs();
    assert_eq!(flag.as_deref(), Some(std::path::Path::new("/x")));
    assert_eq!(probe.as_deref(), Some(std::path::Path::new("main.fab")));
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
        help.contains("--library-home"),
        "check --help must name --library-home:\n{help}"
    );
    assert!(
        help.contains("embedded in the binary"),
        "check --help must name the embedded library home:\n{help}"
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

/// ALA-3 fixture: every verdict kind — annotated ADMISSIBLE (kernel entry),
/// unannotated ADMISSIBLE (unroll note), WOULD-REJECT own-body, WOULD-REJECT
/// via callee closure, NOT-A-TARGET. Latin surface: file mode uses the
/// canonical Latin reader. The kernel entry takes a `numerus<i32>` operand so
/// its `+ 1` store is proven to fit (ruling 25, air1).
const ALL_FOUR_VERDICTS: &str = r#"+++
locale = "la"
+++
@ radix lane "air"
functio computus_purus(i32 x) → numerus {
    redde x + 1
}

functio computus_gyrum() → fractus {
    sit fractus total ← 0.0
    itera ab 0‥10 per 2 sit i {
        total ← total + 1.0
    }
    redde total
}

functio impura(numerus x) → numerus {
    varia numerus total ← x
    total ← total + 1
    redde total
}

functio caller(numerus x) → numerus {
    redde impura(x)
}

functio salve(textus nomen) → textus {
    redde nomen
}
"#;

fn run_faber_stdin(args: &[&str], source: &str) -> std::process::Output {
    let mut command = std::process::Command::new(crate::commands::format_test::faber_binary());
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().expect("spawn faber");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("stdin pipe");
        stdin
            .write_all(source.as_bytes())
            .expect("write stdin source");
    }
    child.wait_with_output().expect("run faber")
}

fn json_row<'a>(rows: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    rows.iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("missing JSON row for {name}: {rows:#?}"))
}

/// ALA-3 pin: `check --air --json` emits one machine-readable object per row
/// (schema_version 2, all four verdict labels including both admissible
/// census tiers, byte spans, via-callee attribution, annotated flag, note),
/// and keeps plain-check exit parity.
#[test]
fn check_air_json_machine_rows_are_versioned_and_complete() {
    let output = run_faber_stdin(&["check", "--air", "--json", "-"], ALL_FOUR_VERDICTS);
    let stdout = String::from_utf8_lossy(&output.stdout);

    let rows = stdout
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|err| panic!("each stdout line must be one JSON object:\n{stdout}\n{err}"));
    assert_eq!(rows.len(), 5, "one row per top-level function:\n{stdout}");

    for row in &rows {
        assert_eq!(
            row["schema_version"], 2,
            "rows are versioned from day one:\n{row}"
        );
        let mut keys = row
            .as_object()
            .expect("every row is a JSON object")
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "annotated",
                "name",
                "note",
                "reasons",
                "schema_version",
                "verdict"
            ],
            "the v2 key set is unchanged from v1:\n{row}"
        );
    }

    let kernel = json_row(&rows, "computus_purus");
    assert_eq!(kernel["verdict"], "ADMISSIBLE-KERNEL");
    assert_eq!(kernel["annotated"], true);
    assert_eq!(kernel["reasons"], serde_json::json!([]));
    assert_eq!(kernel["note"], serde_json::Value::Null);

    let admissible = json_row(&rows, "computus_gyrum");
    assert_eq!(admissible["verdict"], "ADMISSIBLE-ONE-AWAY");
    assert_eq!(admissible["annotated"], false);
    assert_eq!(admissible["reasons"], serde_json::json!([]));
    assert_eq!(
        admissible["note"],
        format!(
            "itera ab unrolls (budget {})",
            radix::module::semantic::passes::air_unroll::UNROLL_BUDGET
        )
    );

    let reject = json_row(&rows, "impura");
    assert_eq!(reject["verdict"], "WOULD-REJECT");
    assert_eq!(reject["annotated"], false);
    assert_eq!(reject["note"], serde_json::Value::Null);
    let reasons = reject["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| { reason["rule"] == "mutable_varia" && reason["via_callee"] == false })
    );
    assert!(
        reasons
            .iter()
            .any(|reason| reason["rule"] == "assignment_mutation")
    );
    for reason in reasons {
        assert!(
            reason["span"]["start"].is_u64() && reason["span"]["end"].is_u64(),
            "reasons carry byte spans:\n{reason}"
        );
        assert!(reason.get("via_callee").is_some());
    }

    let caller = json_row(&rows, "caller");
    assert_eq!(caller["verdict"], "WOULD-REJECT");
    let closure = caller["reasons"]
        .as_array()
        .expect("caller reasons")
        .iter()
        .find(|reason| reason["via_callee"] == true)
        .expect("callee-closure blocker reason");
    assert_eq!(closure["rule"], "blocked by callee `impura`: mutable_varia");

    let not_target = json_row(&rows, "salve");
    assert_eq!(not_target["verdict"], "NOT-A-TARGET");
    assert_eq!(not_target["reasons"], serde_json::json!([]));
    assert_eq!(
        not_target["note"],
        "no numeric/tensor input or output — nothing to differentiate"
    );

    assert!(
        !stdout.contains("air lane advisory report"),
        "machine rows must not carry the human header:\n{stdout}"
    );

    let plain = run_faber_stdin(&["check", "-"], ALL_FOUR_VERDICTS);
    assert_eq!(
        plain.status.code(),
        output.status.code(),
        "--json rows keep plain-check exit parity"
    );
}

/// ALA-3 pin: `--json` without `--air` is rejected at parse time.
#[test]
fn cli_rejects_check_json_without_air() {
    let error =
        Cli::try_parse_from(["faber", "check", "--json"]).expect_err("--json requires --air");
    let rendered = error.to_string();
    assert!(
        rendered.contains("--air"),
        "the rejection must name the required --air flag:\n{rendered}"
    );
}

#[test]
fn test_timeout_flag_parses_seconds() {
    let cli = Cli::try_parse_from(["faber", "test", "--test-timeout", "30"]).expect("parse flag");
    let Some(crate::cli::Command::Test(args)) = cli.command else {
        panic!("expected test subcommand");
    };
    assert_eq!(args.test_timeout, Some(30));
}

#[test]
fn test_timeout_zero_is_rejected_fail_closed() {
    let err =
        Cli::try_parse_from(["faber", "test", "--test-timeout", "0"]).expect_err("0 rejected");
    let rendered = err.to_string();
    assert!(
        rendered.contains("positive"),
        "zero timeout must be rejected fail-closed: {rendered}"
    );
}

#[test]
fn test_stats_flags_parse() {
    let cli = Cli::try_parse_from(["faber", "test", "--stats", "--stats-file", "out.json"])
        .expect("parse stats flags");
    let Some(crate::cli::Command::Test(args)) = cli.command else {
        panic!("expected test subcommand");
    };
    assert!(args.stats);
    assert_eq!(
        args.stats_file.as_deref(),
        Some(std::path::Path::new("out.json"))
    );
}

#[test]
fn manifest_build_target_vocabulary_matches_the_cli() {
    // S09-7: the manifest's `[build] target` vocabulary is exactly the CLI's
    // `-t` vocabulary over canonical spellings — the manifest cannot know
    // less than `faber build`.
    use clap::ValueEnum;
    use radix::program::manifest::manifest_build_target;

    for target in radix::module::codegen::Target::ALL {
        let spelling = target.to_string();
        let manifest_accepts =
            manifest_build_target(Some(&spelling), std::path::Path::new("faber.toml")).is_ok();
        let cli_accepts = FaberCliTarget::from_str(&spelling, false).is_ok();
        assert_eq!(
            manifest_accepts, cli_accepts,
            "manifest and `-t` disagree on '{spelling}'"
        );
    }
}

#[test]
fn cli_parses_format_width_flag() {
    let format = Cli::try_parse_from(["faber", "format", "--width", "80", "main.fab"])
        .expect("parse format --width");
    let Some(crate::cli::Command::Format(args)) = format.command else {
        panic!("expected format subcommand");
    };
    assert_eq!(args.width, Some(80));
    let none = Cli::try_parse_from(["faber", "format", "main.fab"]).expect("parse format");
    let Some(crate::cli::Command::Format(args)) = none.command else {
        panic!("expected format subcommand");
    };
    assert_eq!(args.width, None, "no built-in default width");
    for bad in ["0", "-3", "wide", "4294967296"] {
        assert!(
            Cli::try_parse_from(["faber", "format", "--width", bad, "main.fab"]).is_err(),
            "--width {bad} must be rejected"
        );
    }
}

#[test]
fn format_help_documents_width() {
    let mut command = Cli::command();
    let help = command
        .find_subcommand_mut("format")
        .expect("format subcommand")
        .render_long_help()
        .to_string();
    let words = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(words.contains("--width <N>"), "{help}");
    assert!(words.contains("[format] width"), "{help}");
    assert!(words.contains("with neither set nothing wraps"), "{help}");
}
