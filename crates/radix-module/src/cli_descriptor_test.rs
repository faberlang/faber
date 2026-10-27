//! Companion tests for `cli_descriptor` (encoder-side golden byte fixtures).
//!
//! Radix owns only the encoder; the LLVM host runtime (its `cli` module) owns
//! the decoder and is the decode authority. These fixtures pin the v1 wire
//! format byte-for-byte, hand-derived from the byte layout documented in
//! `cli_descriptor.rs`, so a change to the encoder that would break the host
//! fails here first.

use super::*;
use crate::lexer::{Span, Symbol};

fn program_minimal() -> CliProgram {
    CliProgram {
        name: "tool".to_owned(),
        entry_args: "args".to_owned(),
        mode: CliMode::SingleCommand,
        version: None,
        description: None,
        global_options: Vec::new(),
        global_operands: Vec::new(),
        options: Vec::new(),
        operands: Vec::new(),
        commands: Vec::new(),
        exit: None,
    }
}

fn program_records() -> CliProgram {
    CliProgram {
        name: "tool".to_owned(),
        entry_args: "args".to_owned(),
        mode: CliMode::Subcommand,
        version: Some("1.2".to_owned()),
        description: None,
        global_options: Vec::new(),
        global_operands: Vec::new(),
        options: vec![CliOption {
            binding: "count".to_owned(),
            binding_symbol: Symbol(1),
            ty: CliType::Numerus,
            short: None,
            long: Some("count".to_owned()),
            description: None,
            global: false,
            default: Some(CliDefault::Integer(3)),
            flag: false,
            span: Span::default(),
        }],
        operands: vec![CliOperand {
            binding: "input".to_owned(),
            binding_symbol: Symbol(2),
            ty: CliType::Textus,
            rest: false,
            description: None,
            global: false,
            default: None,
            span: Span::default(),
        }],
        commands: vec![CliCommand {
            path: vec!["greet".to_owned()],
            module_path: None,
            function: "greet".to_owned(),
            function_symbol: Symbol(3),
            args_binding: Some("args".to_owned()),
            aliases: vec!["g".to_owned()],
            description: None,
            options: Vec::new(),
            operands: Vec::new(),
            span: Span::default(),
        }],
        exit: Some(CliExit::Fixed(3)),
    }
}

/// Golden v1 encoding of [`program_minimal`].
#[rustfmt::skip]
const GOLDEN_MINIMAL: &[u8] = &[
    b'F', b'C', b'L', b'I',                               // magic
    CLI_DESCRIPTOR_VERSION,                               // version
    0,                                                    // mode SingleCommand
    0,                                                    // has_version
    0,                                                    // has_description
    4, 0, b't', b'o', b'o', b'l',                         // name "tool"
    0,                                                    // exit tag none
    0, 0,                                                 // global_options count 0
    0, 0,                                                 // global_operands count 0
    0, 0,                                                 // options count 0
    0, 0,                                                 // operands count 0
    0, 0,                                                 // commands count 0
];

/// Golden v1 encoding of [`program_records`].
#[rustfmt::skip]
const GOLDEN_RECORDS: &[u8] = &[
    b'F', b'C', b'L', b'I',                               // magic
    CLI_DESCRIPTOR_VERSION,                               // version
    1,                                                    // mode Subcommand
    1,                                                    // has_version
    0,                                                    // has_description
    4, 0, b't', b'o', b'o', b'l',                         // name "tool"
    3, 0, b'1', b'.', b'2',                               // version "1.2"
    1,                                                    // exit tag fixed
    3, 0, 0, 0, 0, 0, 0, 0,                               //   i64 3
    0, 0,                                                 // global_options count 0
    0, 0,                                                 // global_operands count 0
    1, 0,                                                 // options count 1
    1,                                                    //   ty numerus
    0,                                                    //   flag false
    0,                                                    //   short none
    1,                                                    //   long some
    5, 0, b'c', b'o', b'u', b'n', b't',                   //     "count"
    0,                                                    //   description none
    1,                                                    //   default some
    1,                                                    //     tag integer
    3, 0, 0, 0, 0, 0, 0, 0,                               //     i64 3
    5, 0, b'c', b'o', b'u', b'n', b't',                   //   binding "count"
    1, 0,                                                 // operands count 1
    0,                                                    //   ty textus
    0,                                                    //   rest false
    0,                                                    //   description none
    0,                                                    //   default none
    5, 0, b'i', b'n', b'p', b'u', b't',                   //   binding "input"
    1, 0,                                                 // commands count 1
    1,                                                    //   path len 1
    5, 0, b'g', b'r', b'e', b'e', b't',                   //     "greet"
    1,                                                    //   aliases len 1
    1, 0, b'g',                                           //     "g"
    0,                                                    //   description none
    5, 0, b'g', b'r', b'e', b'e', b't',                   //   function "greet"
    1, 4, 0, b'a', b'r', b'g', b's',                      //   args_binding some "args"
    0, 0,                                                 //   options count 0
    0, 0,                                                 //   operands count 0
];

#[test]
fn encodes_minimal_program_to_golden_bytes() {
    assert_eq!(serialize_cli_descriptor(&program_minimal()), GOLDEN_MINIMAL);
}

#[test]
fn encodes_records_to_golden_bytes() {
    assert_eq!(serialize_cli_descriptor(&program_records()), GOLDEN_RECORDS);
}

#[test]
fn serialization_is_deterministic() {
    let program = program_records();
    assert_eq!(serialize_cli_descriptor(&program), serialize_cli_descriptor(&program));
}
