//! Runner pins for `none` as a set element and map key.

use super::lib_test::evaluate_collection_call;
use crate::Value;
use crate::value::MapKey;
use radix_mir::{MirCollectionOp, MirIntrinsic, MirOperand, MirRuntimeCall, MirType, MirValueId};
use radix_types::TypeId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::rc::Rc;

fn call(op: MirCollectionOp, args: usize) -> MirRuntimeCall {
    MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(op),
        args: (0..args)
            .map(|index| MirOperand::Value(MirValueId(u32::try_from(index).expect("fits"))))
            .collect(),
        return_ty: MirType::semantic(TypeId(0)),
    }
}

fn run(op: MirCollectionOp, inputs: Vec<Value>) -> Value {
    let args = inputs.len();
    let bound = inputs
        .into_iter()
        .enumerate()
        .map(|(index, value)| (MirValueId(u32::try_from(index).expect("fits")), value))
        .collect();
    evaluate_collection_call(&call(op, args), bound).expect("collection call")
}

#[test]
fn none_is_a_set_element() {
    let set = Rc::new(RefCell::new(FxHashSet::default()));
    let receiver = || Value::Set(set.clone());
    run(MirCollectionOp::Append, vec![receiver(), Value::Int(5)]);
    run(MirCollectionOp::Append, vec![receiver(), Value::Nil]);
    run(MirCollectionOp::Append, vec![receiver(), Value::Int(5)]);
    run(MirCollectionOp::Append, vec![receiver(), Value::Nil]);
    assert_eq!(
        run(MirCollectionOp::Length, vec![receiver()]),
        Value::Int(2)
    );
    assert_eq!(
        run(MirCollectionOp::Contains, vec![receiver(), Value::Nil]),
        Value::Bool(true)
    );
    // The none element is stored as the none key and reads back as none.
    assert!(set.borrow().contains(&MapKey::Unit));
    assert_eq!(MapKey::Unit.to_value(), Value::Nil);
    assert_eq!(
        run(MirCollectionOp::Delete, vec![receiver(), Value::Nil]),
        Value::Bool(true)
    );
    assert_eq!(
        run(MirCollectionOp::Length, vec![receiver()]),
        Value::Int(1)
    );
    assert_eq!(
        run(MirCollectionOp::Contains, vec![receiver(), Value::Nil]),
        Value::Bool(false)
    );
}

#[test]
fn none_is_a_map_key() {
    let map = Rc::new(RefCell::new(FxHashMap::default()));
    map.borrow_mut()
        .insert(MapKey::Unit, Value::Str("none".to_owned()));
    map.borrow_mut()
        .insert(MapKey::UInt(5), Value::Str("five".to_owned()));
    let receiver = || Value::Map(map.clone());
    assert_eq!(
        run(MirCollectionOp::Length, vec![receiver()]),
        Value::Int(2)
    );
    assert_eq!(
        run(MirCollectionOp::Delete, vec![receiver(), Value::Nil]),
        Value::Bool(true)
    );
    assert_eq!(
        run(MirCollectionOp::Length, vec![receiver()]),
        Value::Int(1)
    );
}
