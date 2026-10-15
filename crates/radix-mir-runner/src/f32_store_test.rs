//! ef57ab92-2: a value has no width until it is stored. A float stored into a
//! bare `f32` local or temp rounds to the nearest binary32; an `f64` cell
//! keeps the float as written.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlock, MirBlockId, MirConstant, MirFunction, MirFunctionId, MirLocal, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirProgram, MirStatement, MirStatementKind,
    MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType, MirValidationContext, MirValue,
    MirValueId, MirValueKind,
};
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};

fn float_ty(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Fractus, width)
}

/// Assign the float constant `value` into `place`.
fn assign_float(place: MirPlace, value: f64) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place,
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(value))),
                ty: MirType::semantic(TypeId(0)),
                span: Span::default(),
            },
        },
        span: Span::default(),
    }
}

/// Run one `Assign` of `value` into a single cell of type `cell_ty` (a local,
/// or a temp when `temp` is set) and read the cell back through a place
/// operand.
fn store_and_read(cell_ty: Type, temp: bool, value: f64) -> Value {
    let mut types = TypeTable::new();
    let ty = types.intern(cell_ty);
    store_and_read_in(types, ty, temp, value)
}

/// [`store_and_read`] over an already-built type table and cell type id.
fn store_and_read_in(types: TypeTable, ty: TypeId, temp: bool, value: f64) -> Value {
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let place = if temp {
        MirPlace::temp(MirTempId(0))
    } else {
        MirPlace::local(MirLocalId(0))
    };
    let statement = assign_float(place.clone(), value);
    let mut function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    if temp {
        function.temps.push(MirTemp {
            id: MirTempId(0),
            ty: MirType::semantic(ty),
            span: Span::default(),
        });
    } else {
        function.locals.push(MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: MirType::semantic(ty),
            mutable: true,
            span: Span::default(),
        });
    }
    function.blocks.push(MirBlock {
        id: MirBlockId(0),
        statements: vec![statement.clone()],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    });
    let mut program = MirProgram::new();
    program.functions.push(function);
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.execute_statement(&statement).expect("assign");
    runner
        .eval_operand(&MirOperand::Place(place), Span::default())
        .expect("read back")
}

#[test]
fn f32_local_store_rounds_to_binary32() {
    let read = store_and_read(float_ty(NumericWidth::F32), false, 0.1);
    assert_eq!(read, Value::Float(f64::from(0.1_f64 as f32)));
    assert_ne!(read, Value::Float(0.1));
}

#[test]
fn f64_local_store_keeps_the_float() {
    let read = store_and_read(float_ty(NumericWidth::F64), false, 0.1);
    assert_eq!(read, Value::Float(0.1));
}

#[test]
fn f32_temp_store_rounds_to_binary32() {
    let read = store_and_read(float_ty(NumericWidth::F32), true, 0.1);
    assert_eq!(read, Value::Float(f64::from(0.1_f64 as f32)));
    assert_ne!(read, Value::Float(0.1));
}

#[test]
fn f32_alias_cell_store_rounds_to_binary32() {
    let mut types = TypeTable::new();
    let f32_ty = types.intern(float_ty(NumericWidth::F32));
    let alias = types.intern(Type::Alias(radix_types::DefId(9_001), f32_ty));
    assert!(is_bare_f32(&types, alias));
    let read = store_and_read_in(types, alias, false, 0.1);
    assert_eq!(read, Value::Float(f64::from(0.1_f64 as f32)));
}

#[test]
fn bare_f32_flag_excludes_f64_and_policy_cells() {
    let mut types = TypeTable::new();
    let f32_ty = types.intern(float_ty(NumericWidth::F32));
    assert!(is_bare_f32(&types, f32_ty));
    let f64_ty = types.intern(float_ty(NumericWidth::F64));
    assert!(!is_bare_f32(&types, f64_ty));
    let policy_ty = types.intern(Type::PolicyFloat(
        radix_types::FloatPolicy::Saturating,
        NumericWidth::F32,
    ));
    assert!(!is_bare_f32(&types, policy_ty));
    let read = store_and_read_in(types, policy_ty, false, 0.1);
    assert_eq!(read, Value::Float(0.1));
}

/// A callee `f32 id(f32 x) → x` bound to the constant `0.1` and run through
/// `invoke_call`: argument binding is a store into the parameter cell.
#[test]
fn f32_parameter_round_trip_rounds_to_binary32() {
    let mut types = TypeTable::new();
    let f32_ty = types.intern(float_ty(NumericWidth::F32));
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let span = Span::default();
    let scalar = MirType::semantic(f32_ty);
    let callee = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: scalar,
            mode: MirParamMode::Owned,
            span,
        }],
        locals: vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: scalar,
            mutable: false,
            span,
        }],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(0),
                )))),
                span,
            },
            span,
        }],
        return_ty: scalar,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    };
    let caller = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: scalar,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    };
    let mut program = MirProgram::new();
    program.functions.push(caller);
    program.functions.push(callee);
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let outcome = runner
        .invoke_call(
            &MirCallee::Function(MirFunctionId(1)),
            &[MirOperand::Constant(MirConstant::Float(0.1))],
            span,
        )
        .expect("call");
    let CallOutcome::Ok(value) = outcome else {
        panic!("call raised an error value");
    };
    assert_eq!(value, Value::Float(f64::from(0.1_f64 as f32)));
    assert_ne!(value, Value::Float(0.1));
}
