//! A non-dynamic (`json`) source converting into a genus applies the declared
//! field defaults of a nested genus, not the zero value of its field type.

use crate::host::BufferHost;
use crate::{MapKey, Runner, Value};
use radix_hir::DefId;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirConstant, MirConversion, MirConversionFlavor, MirNamedOperand, MirOperand, MirProgram,
    MirStructFieldDefaults, MirType, MirValidationContext,
};
use radix_types::{Primitive, Type, TypeTable};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn nested_genus_field_absent_from_json_takes_its_declared_default() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let inner_field = interner.intern("inner");
    let count_field = interner.intern("count");
    let inner_def = DefId(41);
    let outer_def = DefId(42);
    let json = types.primitive(Primitive::Json);
    let numerus = types.primitive(Primitive::Numerus);
    let inner_ty = types.intern(Type::Struct(inner_def));
    let outer_ty = types.intern(Type::Struct(outer_def));
    let mut context = MirValidationContext::new(&types);
    context.struct_fields.insert(
        inner_def,
        [(count_field, MirType::semantic(numerus))]
            .into_iter()
            .collect(),
    );
    context.struct_fields.insert(
        outer_def,
        [(inner_field, MirType::semantic(inner_ty))]
            .into_iter()
            .collect(),
    );
    let program = MirProgram::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);

    let empty_inner = Value::Map(Rc::new(RefCell::new(FxHashMap::default())));
    let mut object = FxHashMap::default();
    object.insert(MapKey::Str("inner".to_owned()), empty_inner);
    let source = Value::Map(Rc::new(RefCell::new(object)));

    let converted = runner
        .eval_runtime_convert(
            &MirConversion {
                store: None,
                flavor: MirConversionFlavor::Runtime,
                source_ty: MirType::semantic(json),
                target_ty: MirType::semantic(outer_ty),
                params: vec![],
                recovery: None,
                struct_defaults: vec![MirStructFieldDefaults {
                    def_id: inner_def,
                    fields: vec![MirNamedOperand {
                        name: count_field,
                        value: MirOperand::Constant(MirConstant::Int(5)),
                    }],
                }],
            },
            source,
            Span::default(),
        )
        .expect("json to genus conversion");

    let Value::Struct(def, outer) = converted else {
        panic!("expected a genus value, got {converted:?}");
    };
    assert_eq!(def, outer_def);
    let Some(Value::Struct(_, inner)) = outer.borrow().get(&inner_field).cloned() else {
        panic!("expected the nested genus value");
    };
    assert!(
        matches!(inner.borrow().get(&count_field), Some(Value::Int(5))),
        "nested field: {:?}",
        inner.borrow().get(&count_field)
    );
}
