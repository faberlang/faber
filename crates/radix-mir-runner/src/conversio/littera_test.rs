//! Runner pins for the D10.6 `littera` text rows and the `Code` hint. Each
//! failable row fails closed without a `⊥` default (the error is the proof).

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{TypeId, TypeTable};

#[derive(Clone, Copy)]
enum Row {
    ToLittera,
    LitteraToNumerus,
    ToAscii,
}

fn eval(row: Row, value: Value, code: bool) -> Result<Value, RunnerError> {
    let type_table = TypeTable::new();
    let mut interner = Interner::new();
    let code_sym = interner.intern("Code");
    let params = if code { vec![code_sym] } else { Vec::new() };
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    match row {
        Row::ToLittera => runner.convert_to_littera(value, &params, None, Span::default()),
        Row::LitteraToNumerus => {
            let ch = littera::littera_value(&value).expect("littera carrier");
            runner.convert_littera_to_numerus(ch, &params, None, Span::default())
        }
        Row::ToAscii => runner.convert_to_ascii(value, None, Span::default()),
    }
}

fn ok(row: Row, value: Value, code: bool) -> Value {
    eval(row, value, code).expect("conversion succeeds")
}

fn fails(row: Row, value: Value, code: bool) -> bool {
    eval(row, value, code).is_err()
}

#[test]
fn numerus_to_ascii_is_digits() {
    assert!(matches!(ok(Row::ToAscii, Value::Int(42), false), Value::Str(s) if s == "42"));
    assert!(matches!(ok(Row::ToAscii, Value::Int(-7), false), Value::Str(s) if s == "-7"));
}

#[test]
fn numerus_to_littera_is_one_digit() {
    assert!(matches!(
        ok(Row::ToLittera, Value::Int(7), false),
        Value::Char('7')
    ));
    assert!(matches!(
        ok(Row::ToLittera, Value::UInt(0), false),
        Value::Char('0')
    ));
    assert!(fails(Row::ToLittera, Value::Int(42), false));
    assert!(fails(Row::ToLittera, Value::Int(-1), false));
}

#[test]
fn textus_to_littera_needs_exactly_one_scalar() {
    assert!(matches!(
        ok(Row::ToLittera, Value::Str("ü".into()), false),
        Value::Char('ü')
    ));
    assert!(fails(Row::ToLittera, Value::Str(String::new()), false));
    assert!(fails(Row::ToLittera, Value::Str("ab".into()), false));
}

#[test]
fn littera_to_numerus_parses_the_digit() {
    assert!(matches!(
        ok(Row::LitteraToNumerus, Value::Char('7'), false),
        Value::Int(7)
    ));
    assert!(matches!(
        ok(Row::LitteraToNumerus, Value::Ascii(b'3'), false),
        Value::Int(3)
    ));
    assert!(fails(Row::LitteraToNumerus, Value::Char('x'), false));
}

#[test]
fn code_hint_reads_the_code_point() {
    assert!(matches!(
        ok(Row::LitteraToNumerus, Value::Char('A'), true),
        Value::UInt(65)
    ));
    assert!(matches!(
        ok(Row::LitteraToNumerus, Value::Char('👍'), true),
        Value::UInt(0x1F44D)
    ));
}

#[test]
fn code_hint_builds_the_scalar() {
    assert!(matches!(
        ok(Row::ToLittera, Value::Int(65), true),
        Value::Char('A')
    ));
    assert!(fails(Row::ToLittera, Value::Int(0xD800), true));
    assert!(fails(Row::ToLittera, Value::Int(0x11_0000), true));
    assert!(fails(Row::ToLittera, Value::Int(-1), true));
}
