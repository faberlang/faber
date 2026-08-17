//! Thin binary entry point for the compiler-developer `radix` tool.
//!
//! Clap shapes live in `radix_module::tool::cli`; handlers live in `radix_module::tool::commands`.
//! This file is only the parse boundary plus subcommand dispatch.
//!
//! INVARIANT
//! =========
//! Every arm should delegate immediately to `tool` and avoid embedding compiler
//! phase behavior in the binary crate.

use clap::Parser;
use radix_module::tool::{self, CheckCommand, DiagnosticMode, EmitCommand, RadixCli, RadixCommand, VerifyCommand};

fn diagnostic_mode(expanded: bool) -> DiagnosticMode {
    if expanded {
        DiagnosticMode::Diagnostics
    } else {
        DiagnosticMode::Normal
    }
}

fn validate_deny_codes(codes: &[String]) {
    for code in codes {
        if !radix_module::is_known_diagnostic_code(code) {
            eprintln!("error: unknown diagnostic code '{code}' passed to --deny");
            std::process::exit(1);
        }
    }
}

fn main() {
    let cli = RadixCli::parse();

    match cli.command {
        RadixCommand::Targets => tool::cmd_targets(),
        RadixCommand::Abi(args) => tool::cmd_abi(args),
        RadixCommand::Lex(args) => tool::cmd_lex(&args.input),
        RadixCommand::Parse(args) => tool::cmd_parse(&args.input),
        RadixCommand::Hir(args) => tool::cmd_hir(&args.input, args.load),
        #[cfg(feature = "mir-core")]
        RadixCommand::Mir(args) => tool::cmd_mir(&args.input),
        #[cfg(not(feature = "mir-core"))]
        RadixCommand::Mir(_) => {
            eprintln!("error: `radix mir` requires the MIR lane; rebuild with --features mir-core");
            std::process::exit(1);
        }
        RadixCommand::CliIr(args) => tool::cmd_cli_ir(&args.input),
        RadixCommand::Check(args) => {
            validate_deny_codes(&args.deny);
            tool::cmd_check(CheckCommand {
                input: args.input,
                package: args.package,
                permissive: args.permissive,
                deny_warnings: args.deny_warnings,
                deny_codes: args.deny,
                diagnostic_mode: diagnostic_mode(args.diagnostics),
                diagnostics_locale_pack: args.diagnostics_locale_pack,
                locale: args.locale,
                diagnostics_locale: None,
            });
        }
        RadixCommand::Verify(args) => tool::cmd_verify(VerifyCommand { input: args.input, package: args.package }),
        RadixCommand::Emit(args) => tool::cmd_emit(EmitCommand {
            input: args.input,
            package: args.package,
            target: args.target.into(),
            reflection: args.reflection,
            #[cfg(feature = "mir-llvm")]
            cuda_descriptor: args.cuda_descriptor,
            output: args.output,
            diagnostic_mode: diagnostic_mode(args.diagnostics),
            diagnostics_locale_pack: args.diagnostics_locale_pack,
            locale: args.locale,
            diagnostics_locale: None,
            output_mode: args.output_mode.into(),
            module_name: args.module_name,
        }),
    }
}
