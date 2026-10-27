//! Dynamic byte extraction preserves authored numeric-list boxing and native
//! opaque byte carriers. Invalid list elements fail atomically, never coerce.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{MirConversion, MirConversionFlavor, MirValidationContext};
use radix_types::{Primitive, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

fn lista(items: Vec<Value>) -> Value {
    Value::Array(Rc::new(RefCell::new(items)))
}

fn extract(value: Value, recovery: Option<MirOperand>) -> Result<Value, RunnerError> {
    let types = TypeTable::new();
    let source = types.primitive(Primitive::Valor);
    let target = types.primitive(Primitive::Octeti);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let program = MirProgram::new();
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.eval_runtime_convert(
        &MirConversion {
            store: None,
            flavor: MirConversionFlavor::Runtime,
            source_ty: MirType::semantic(source),
            target_ty: MirType::semantic(target),
            params: vec![],
            recovery,
            struct_defaults: vec![],
        },
        value,
        Span::default(),
    )
}

#[test]
fn octeti_valor_extracts_opaque_host_bytes_without_utf8_decoding() {
    let bytes = vec![0, 127, 255];
    assert_eq!(
        extract(Value::Octeti(bytes.clone()), None).expect("opaque bytes"),
        Value::Octeti(bytes)
    );
}

#[test]
fn octeti_valor_extracts_authored_boxed_bytes_without_changing_boxing() {
    let bytes = b"{\"prompt\":\"hi\"}".to_vec();
    let types = TypeTable::new();
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let program = MirProgram::new();
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let boxed = runner
        .convert_to_valor(
            Value::Octeti(bytes.clone()),
            types.primitive(Primitive::Octeti),
            Span::default(),
        )
        .expect("box authored bytes");
    assert_eq!(
        boxed,
        lista(
            bytes
                .iter()
                .map(|byte| Value::Int(i64::from(*byte)))
                .collect()
        )
    );
    assert_eq!(
        extract(boxed, None).expect("unbox bytes"),
        Value::Octeti(bytes)
    );
}

#[test]
fn octeti_valor_extracts_empty_and_boundary_byte_lists() {
    for bytes in [vec![], vec![0, 127, 255]] {
        let boxed = lista(
            bytes
                .iter()
                .map(|byte| Value::Int(i64::from(*byte)))
                .collect(),
        );
        assert_eq!(
            extract(boxed, None).expect("byte list"),
            Value::Octeti(bytes)
        );
    }
}

#[test]
fn octeti_valor_rejects_non_bytes_and_bad_lists_atomically() {
    for invalid in [
        Value::Nil,
        Value::Str("abc".to_owned()),
        Value::Bool(true),
        lista(vec![Value::Int(0), Value::Int(-1)]),
        lista(vec![Value::Int(0), Value::Int(256)]),
        lista(vec![Value::Int(0), Value::Float(1.0)]),
        lista(vec![Value::Int(0), Value::Str("1".to_owned())]),
        lista(vec![Value::Int(0), Value::Bool(true)]),
    ] {
        let err = extract(invalid, None).expect_err("not a byte carrier");
        assert_eq!(err.issue, "runner_valor_to_list_b8_conversion_failed");
    }
}

#[test]
fn octeti_valor_failed_extraction_honors_recovery() {
    assert_eq!(
        extract(
            lista(vec![Value::Int(7), Value::Int(256)]),
            Some(MirOperand::Constant(MirConstant::Octeti(vec![9])))
        )
        .expect("recover all bytes, not a partial prefix"),
        Value::Octeti(vec![9])
    );
}
