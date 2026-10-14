//! Runner pins for tabula reads selected by the MIR operation's static result type, and for the
//! bounded-lista `est` tag check.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
    MirRuntimeCall, MirType, MirTypeCheckOp, MirValidationContext, MirValueId,
};
use radix_types::{IndexExpr, Primitive, TypeTable};
use rustc_hash::FxHashMap;

fn eval_missing_tabula_key(optional: bool) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let key = type_table.primitive(Primitive::Textus);
    let value = type_table.primitive(Primitive::Numerus);
    let return_ty = if optional {
        type_table.option(value)
    } else {
        value
    };
    let _declared = type_table.map(key, return_ty);
    let interner = Interner::new();
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
        return_ty: MirType::semantic(return_ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().unwrap();
    frame.values.insert(
        MirValueId(0),
        Value::Map(Rc::new(RefCell::new(FxHashMap::default()))),
    );
    frame
        .values
        .insert(MirValueId(1), Value::Str("missing".into()));
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Index),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(return_ty),
    };
    runner.eval_runtime_call(&call, Span::default())
}

#[test]
fn non_optional_tabula_missing_key_is_a_hard_runner_error() {
    let error = eval_missing_tabula_key(false)
        .expect_err("tabula<textus, numerus> missing key must fail hard, not return nihil");
    assert_eq!(error.code, "runner_internal");
    assert_eq!(error.issue, "runner_tabula_missing_key");
    assert_eq!(
        error.args,
        vec![("detail".to_owned(), "tabula missing key".to_owned())]
    );
}

#[test]
fn optional_tabula_missing_key_returns_none() {
    let value = eval_missing_tabula_key(true)
        .expect("tabula<textus, numerus union nihil> missing key must stay legal");
    assert_eq!(value, Value::Nil);
}

/// Evaluate `value est lista<numerus, 8>` (or `non est`) against one runner value.
fn eval_bounded_lista_type_check(value: Value, positive: bool) -> Value {
    let mut type_table = TypeTable::new();
    let numerus = type_table.primitive(Primitive::Numerus);
    let bool_ty = type_table.primitive(Primitive::Bivalens);
    let capacity = type_table.intern_index(IndexExpr::Literal(8));
    let bounded = type_table.bounded_array(numerus, capacity);
    let interner = Interner::new();
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
        return_ty: MirType::semantic(bool_ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), value);
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::TypeCheck(MirTypeCheckOp {
            positive,
            ty: MirType::semantic(bounded),
        }),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(bool_ty),
    };
    runner
        .eval_runtime_call(&call, Span::default())
        .expect("bounded lista type check evaluates")
}

#[test]
fn a_boxed_list_is_a_bounded_lista() {
    let list = Value::Array(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)])));
    assert_eq!(
        eval_bounded_lista_type_check(list.clone(), true),
        Value::Bool(true)
    );
    assert_eq!(
        eval_bounded_lista_type_check(list, false),
        Value::Bool(false)
    );
}

#[test]
fn a_non_list_is_not_a_bounded_lista() {
    assert_eq!(
        eval_bounded_lista_type_check(Value::Int(1), true),
        Value::Bool(false)
    );
}

#[test]
fn fault_code_for_display_name_reads_the_registry() {
    assert_eq!(
        fault_code_for_display_name("IoFault.NotFound"),
        Some("ERR001")
    );
    assert_eq!(
        fault_code_for_display_name("IoFault.Permission"),
        Some("ERR002")
    );
}

#[test]
fn fault_code_for_display_name_rejects_unregistered_shapes() {
    assert_eq!(fault_code_for_display_name("IoFault.NoSuch"), None);
    assert_eq!(fault_code_for_display_name("NotFound"), None);
    assert_eq!(fault_code_for_display_name("Other.NotFound"), None);
}
