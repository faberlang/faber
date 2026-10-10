//! The nearest-rounding collection op (`rotunda` / `⌊x⌉`) sends ties to the
//! even neighbour: `2.5` is 2, `3.5` is 4, `-2.5` is -2.

use super::*;

use crate::host::BufferHost;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirRuntimeCall, MirType,
};
use radix_types::{Primitive, TypeTable};

fn round(input: f64) -> i64 {
    let mut host = BufferHost::default();
    let types = TypeTable::new();
    let interner = Interner::new();
    let context = radix_mir::MirValidationContext::new(&types);
    let mut program = radix_mir::MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(types.primitive(Primitive::Numerus)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), radix_mir::MirBlockId(0));
    runner
        .frames
        .last_mut()
        .expect("round frame")
        .values
        .insert(MirValueId(0), Value::Float(input));
    let value = runner
        .eval_runtime_call(
            &MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::Round),
                args: vec![MirOperand::Value(MirValueId(0))],
                return_ty: MirType::semantic(types.primitive(Primitive::Numerus)),
            },
            Span::default(),
        )
        .expect("round runtime call");
    match value {
        Value::Int(v) => v,
        other => panic!("non-integer rounding result {other:?}"),
    }
}

#[test]
fn nearest_rounding_sends_ties_to_the_even_neighbour_half_even() {
    assert_eq!(round(0.5), 0);
    assert_eq!(round(1.5), 2);
    assert_eq!(round(2.5), 2);
    assert_eq!(round(3.5), 4);
    assert_eq!(round(-2.5), -2);
    assert_eq!(round(-3.5), -4);
}

#[test]
fn nearest_rounding_off_tie_values_round_to_the_nearer_integer_half_even() {
    assert_eq!(round(2.4), 2);
    assert_eq!(round(2.6), 3);
    assert_eq!(round(-2.6), -3);
}
