//! Companion tests for `cli_descriptor` (round-trip wire format).

use super::*;
use crate::lexer::{Interner, Symbol};

fn program_sample(interner: &mut Interner) -> CliProgram {
    let textus = Symbol(1);
    CliProgram {
        name: "exemplum".to_owned(),
        entry_args: "args".to_owned(),
        mode: CliMode::Subcommand,
        version: Some("0.1.0".to_owned()),
        description: Some("Faber CLI framework smoke exemplum".to_owned()),
        global_options: vec![CliOption {
            binding: "verbose".to_owned(),
            binding_symbol: textus,
            ty: CliType::Bivalens,
            short: Some("v".to_owned()),
            long: Some("verbose".to_owned()),
            description: Some("Enable verbose output".to_owned()),
            global: true,
            default: Some(CliDefault::Bool(false)),
            flag: true,
            span: crate::lexer::Span::default(),
        }],
        global_operands: Vec::new(),
        options: Vec::new(),
        operands: Vec::new(),
        commands: vec![
            CliCommand {
                path: vec!["greet".to_owned()],
                module_path: None,
                function: "greet".to_owned(),
                function_symbol: interner.intern("greet"),
                args_binding: Some("args".to_owned()),
                aliases: vec!["g".to_owned()],
                description: Some("Greet by name".to_owned()),
                options: Vec::new(),
                operands: vec![CliOperand {
                    binding: "nomen".to_owned(),
                    binding_symbol: interner.intern("nomen"),
                    ty: CliType::Textus,
                    rest: false,
                    description: Some("Name to greet".to_owned()),
                    global: false,
                    default: None,
                    span: crate::lexer::Span::default(),
                }],
                span: crate::lexer::Span::default(),
            },
            CliCommand {
                path: vec!["version".to_owned()],
                module_path: None,
                function: "version".to_owned(),
                function_symbol: interner.intern("version"),
                args_binding: None,
                aliases: vec!["v".to_owned()],
                description: None,
                options: Vec::new(),
                operands: Vec::new(),
                span: crate::lexer::Span::default(),
            },
        ],
        exit: Some(CliExit::Binding("code".to_owned())),
    }
}

#[test]
fn roundtrips_full_program() {
    let mut interner = Interner::new();
    let program = program_sample(&mut interner);
    let bytes = serialize_cli_descriptor(&program);
    let decoded = parse_cli_descriptor(&bytes).expect("decode");
    assert_eq!(decoded.name, "exemplum");
    assert_eq!(decoded.mode, 1);
    assert_eq!(decoded.version.as_deref(), Some("0.1.0"));
    assert_eq!(decoded.description.as_deref(), Some("Faber CLI framework smoke exemplum"));
    assert_eq!(decoded.exit, DescriptorExit::Binding("code".to_owned()));
    assert_eq!(decoded.global_options.len(), 1);
    let verbose = &decoded.global_options[0];
    assert_eq!(verbose.binding, "verbose");
    assert_eq!(verbose.ty, tags::BIVALENS);
    assert!(verbose.flag);
    assert_eq!(verbose.short.as_deref(), Some("v"));
    assert_eq!(verbose.long.as_deref(), Some("verbose"));
    assert_eq!(verbose.default.as_ref().map(|d| d.tag), Some(default_tags::BOOL));
    assert_eq!(verbose.default.as_ref().and_then(|d| d.boolean), Some(false));
    assert_eq!(decoded.commands.len(), 2);
    let greet = &decoded.commands[0];
    assert_eq!(greet.path, vec!["greet".to_owned()]);
    assert_eq!(greet.aliases, vec!["g".to_owned()]);
    assert_eq!(greet.function, "greet");
    assert_eq!(greet.args_binding.as_deref(), Some("args"));
    assert_eq!(greet.operands.len(), 1);
    assert_eq!(greet.operands[0].binding, "nomen");
    assert_eq!(greet.operands[0].ty, tags::TEXTUS);
    assert!(!greet.operands[0].rest);
}

#[test]
fn serialization_is_deterministic() {
    let mut interner = Interner::new();
    let program = program_sample(&mut interner);
    let first = serialize_cli_descriptor(&program);
    let second = serialize_cli_descriptor(&program);
    assert_eq!(first, second);
}

#[test]
fn decode_rejects_corruption() {
    let mut interner = Interner::new();
    let program = program_sample(&mut interner);
    let bytes = serialize_cli_descriptor(&program);
    assert!(parse_cli_descriptor(&bytes).is_ok());
    assert!(parse_cli_descriptor(&bytes[..bytes.len() - 1]).is_err(), "truncated");
    let mut bad_version = bytes.clone();
    bad_version[4] = 99;
    assert!(parse_cli_descriptor(&bad_version).is_err(), "bad version");
    let mut bad_magic = bytes.clone();
    bad_magic[0] = b'X';
    assert!(parse_cli_descriptor(&bad_magic).is_err(), "bad magic");
}

#[test]
fn single_command_roundtrip_with_defaults() {
    let mut interner = Interner::new();
    let program = CliProgram {
        name: "operandus-smoke".to_owned(),
        entry_args: "args".to_owned(),
        mode: CliMode::SingleCommand,
        version: None,
        description: None,
        global_options: Vec::new(),
        global_operands: Vec::new(),
        options: vec![CliOption {
            binding: "count".to_owned(),
            binding_symbol: interner.intern("count"),
            ty: CliType::Numerus,
            short: None,
            long: Some("count".to_owned()),
            description: Some("Repeat count".to_owned()),
            global: false,
            default: Some(CliDefault::Integer(3)),
            flag: false,
            span: crate::lexer::Span::default(),
        }],
        operands: vec![
            CliOperand {
                binding: "input".to_owned(),
                binding_symbol: interner.intern("input"),
                ty: CliType::Textus,
                rest: false,
                description: Some("Input path".to_owned()),
                global: false,
                default: None,
                span: crate::lexer::Span::default(),
            },
            CliOperand {
                binding: "files".to_owned(),
                binding_symbol: interner.intern("files"),
                ty: CliType::ListaTextus,
                rest: true,
                description: Some("Extra files".to_owned()),
                global: false,
                default: None,
                span: crate::lexer::Span::default(),
            },
        ],
        commands: Vec::new(),
        exit: Some(CliExit::Fixed(7)),
    };
    let bytes = serialize_cli_descriptor(&program);
    let decoded = parse_cli_descriptor(&bytes).expect("decode");
    assert_eq!(decoded.mode, 0);
    assert_eq!(decoded.options.len(), 1);
    let count = &decoded.options[0];
    assert_eq!(count.ty, tags::NUMERUS);
    assert_eq!(count.default.as_ref().and_then(|d| d.integer), Some(3));
    assert_eq!(decoded.operands.len(), 2);
    assert!(decoded.operands[1].rest);
    assert_eq!(decoded.operands[1].ty, tags::LISTA_TEXTUS);
    assert_eq!(decoded.exit, DescriptorExit::Fixed(7));
}
