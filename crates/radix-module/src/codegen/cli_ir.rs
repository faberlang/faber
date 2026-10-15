//! Convert package CLI IR into the shared leaf CLI IR.
//!
//! The conversion is target-neutral: it maps `crate::cli` program facts into
//! `radix_codegen_shared::cli_ir` without depending on any backend.

use crate::cli::{CliCommand, CliDefault, CliExit, CliMode, CliOperand, CliOption, CliProgram, CliType};
use radix_codegen_shared::cli_ir;

/// Convert package CLI IR into the leaf CLI IR.
pub fn to_cli_ir(program: &CliProgram) -> cli_ir::CliProgram {
    cli_ir::CliProgram {
        name: program.name.clone(),
        entry_args: program.entry_args.clone(),
        mode: match program.mode {
            CliMode::NotCli => cli_ir::CliMode::NotCli,
            CliMode::SingleCommand => cli_ir::CliMode::SingleCommand,
            CliMode::Subcommand => cli_ir::CliMode::Subcommand,
        },
        version: program.version.clone(),
        description: program.description.clone(),
        global_options: program.global_options.iter().map(to_cli_option).collect(),
        global_operands: program.global_operands.iter().map(to_cli_operand).collect(),
        options: program.options.iter().map(to_cli_option).collect(),
        operands: program.operands.iter().map(to_cli_operand).collect(),
        commands: program.commands.iter().map(to_cli_command).collect(),
        exit: program.exit.as_ref().map(to_cli_exit),
    }
}

fn to_cli_exit(exit: &CliExit) -> cli_ir::CliExit {
    match exit {
        CliExit::Fixed(code) => cli_ir::CliExit::Fixed(*code),
        CliExit::Binding(name) => cli_ir::CliExit::Binding(name.clone()),
        CliExit::Field { object, field } => cli_ir::CliExit::Field { object: object.clone(), field: field.clone() },
        CliExit::Unsupported => cli_ir::CliExit::Unsupported,
    }
}

fn to_cli_command(cmd: &CliCommand) -> cli_ir::CliCommand {
    cli_ir::CliCommand {
        path: cmd.path.clone(),
        module_path: cmd.module_path.clone(),
        function: cmd.function.clone(),
        function_symbol: cmd.function_symbol,
        args_binding: cmd.args_binding.clone(),
        aliases: cmd.aliases.clone(),
        description: cmd.description.clone(),
        options: cmd.options.iter().map(to_cli_option).collect(),
        operands: cmd.operands.iter().map(to_cli_operand).collect(),
        span: cmd.span,
    }
}

fn to_cli_option(opt: &CliOption) -> cli_ir::CliOption {
    cli_ir::CliOption {
        binding: opt.binding.clone(),
        binding_symbol: opt.binding_symbol,
        ty: to_cli_type(&opt.ty),
        short: opt.short.clone(),
        long: opt.long.clone(),
        description: opt.description.clone(),
        global: opt.global,
        default: opt.default.as_ref().map(to_cli_default),
        flag: opt.flag,
        span: opt.span,
    }
}

fn to_cli_operand(op: &CliOperand) -> cli_ir::CliOperand {
    cli_ir::CliOperand {
        binding: op.binding.clone(),
        binding_symbol: op.binding_symbol,
        ty: to_cli_type(&op.ty),
        rest: op.rest,
        description: op.description.clone(),
        global: op.global,
        default: op.default.as_ref().map(to_cli_default),
        span: op.span,
    }
}

fn to_cli_type(ty: &CliType) -> cli_ir::CliType {
    match ty {
        CliType::Textus => cli_ir::CliType::Textus,
        CliType::Numerus => cli_ir::CliType::Numerus,
        CliType::Fractus => cli_ir::CliType::Fractus,
        CliType::Bivalens => cli_ir::CliType::Bivalens,
        CliType::Octeti => cli_ir::CliType::Octeti,
        CliType::Ignotum => cli_ir::CliType::Ignotum,
        CliType::ListaTextus => cli_ir::CliType::ListaTextus,
        CliType::ListaNumerus => cli_ir::CliType::ListaNumerus,
    }
}

fn to_cli_default(default: &CliDefault) -> cli_ir::CliDefault {
    match default {
        CliDefault::Text(s) => cli_ir::CliDefault::Text(s.clone()),
        CliDefault::Integer(n) => cli_ir::CliDefault::Integer(*n),
        CliDefault::Float(n) => cli_ir::CliDefault::Float(*n),
        CliDefault::Bool(b) => cli_ir::CliDefault::Bool(*b),
        CliDefault::Nil => cli_ir::CliDefault::Nil,
        CliDefault::Expr(s) => cli_ir::CliDefault::Expr(s.clone()),
    }
}
