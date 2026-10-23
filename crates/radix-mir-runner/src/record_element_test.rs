//! Runner pins for a record-element tensor: index, then a field read.
//!
//! A `tensor<Block, [n]>` keeps its non-scalar elements in
//! `DenseStorage::Values`, so the reference execution reads one element as the
//! same `Value::Struct` a plain binding holds. Both projection orders over such
//! an element (`index` then `field`, and `field` then `index`) must therefore
//! reach the record's fields through `collection_index` and `project_field`.

use super::*;
use crate::value::TensorValue;
use radix_hir::DefId;
use radix_lexer::{Interner, Symbol};
use radix_mir::{
    MirBlockId, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram, MirProjection,
    MirType, MirValidationContext,
};
use radix_types::{TypeId, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

fn with_runner<T>(body: impl FnOnce(&mut crate::Runner<'_, crate::host::BufferHost>) -> T) -> T {
    let type_table = TypeTable::new();
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
    body(&mut runner)
}

fn block(scale: Symbol, values: Symbol, def: DefId, scale_value: f64, cells: &[i64]) -> Value {
    let mut fields = rustc_hash::FxHashMap::default();
    fields.insert(scale, Value::Float(scale_value));
    fields.insert(
        values,
        Value::Array(Rc::new(RefCell::new(
            cells.iter().copied().map(Value::Int).collect(),
        ))),
    );
    Value::Struct(def, Rc::new(RefCell::new(fields)))
}

fn index(at: i64) -> MirProjection {
    MirProjection::Index(MirOperand::Constant(MirConstant::Int(at)))
}

fn field(name: Symbol) -> MirProjection {
    MirProjection::Field(name)
}

fn read(
    runner: &mut crate::Runner<'_, crate::host::BufferHost>,
    receiver: &Value,
    projections: &[MirProjection],
) -> Value {
    let mut current = receiver.clone();
    for projection in projections {
        current = runner
            .project(current, projection, Span::default())
            .expect("record-element projection must resolve in the reference runner");
    }
    current
}

#[test]
fn record_tensor_index_then_field_read_in_reference() {
    let mut interner = Interner::new();
    let scale = interner.intern("scale");
    let values = interner.intern("values");
    let def = DefId(7);
    // Both elements carry an f32 `scale` and a `lista<numerus>` field, so the
    // dense backing stores a struct per element rather than a scalar lane.
    let block0 = block(scale, values, def, 1.5, &[3, 4]);
    let block1 = block(scale, values, def, 2.5, &[3, 4]);
    let tensor = TensorValue::try_contiguous(vec![block0, block1], vec![2])
        .expect("fixture tensor shape must have representable strides");
    let receiver = Value::Tensor(Rc::new(RefCell::new(tensor)));

    with_runner(|runner| {
        // index 1, then read `scale`.
        assert_eq!(
            read(runner, &receiver, &[index(1), field(scale)]),
            Value::Float(2.5)
        );
        // index 0 keeps the first element's own field, not the last lane's.
        assert_eq!(
            read(runner, &receiver, &[index(0), field(scale)]),
            Value::Float(1.5)
        );
        // read `values`, then index the lista: the field is the real lista.
        assert_eq!(
            read(runner, &receiver, &[index(1), field(values), index(0)]),
            Value::Int(3)
        );
        assert_eq!(
            read(runner, &receiver, &[index(1), field(values), index(1)]),
            Value::Int(4)
        );
        // field read first, then index (the element came from the tensor).
        let element = read(runner, &receiver, &[index(1)]);
        assert_eq!(
            read(runner, &element, &[field(values), index(0)]),
            Value::Int(3)
        );
    });
}
