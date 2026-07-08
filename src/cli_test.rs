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
fn cli_parses_repl_subcommand() {
    let cli = Cli::try_parse_from(["faber", "repl"]).expect("parse repl");
    assert!(cli.eval_source.is_none());
    assert!(matches!(cli.command, Some(crate::cli::Command::Repl(_))));
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
fn cli_leaves_build_target_unset_when_omitted() {
    let build = Cli::try_parse_from(["faber", "build", "pkg"]).expect("parse build");
    let Some(crate::cli::Command::Build(args)) = build.command else {
        panic!("expected build subcommand");
    };
    assert_eq!(args.target, None);
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
fn cli_parses_reader_locale_on_explain() {
    let explain = Cli::try_parse_from([
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
fn cli_parses_verify_subcommand() {
    let cli = Cli::try_parse_from(["faber", "verify", "main.fab"]).expect("parse verify");
    let Some(crate::cli::Command::Verify(args)) = cli.command else {
        panic!("expected verify subcommand");
    };
    assert!(!args.package);
    assert_eq!(args.input, vec!["main.fab"]);
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
