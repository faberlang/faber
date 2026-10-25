//! Runner pins for `tabula<K, V> ↦ lista<T>` (rn-tab).
//!
//! The law (conversio-collection-coverage goal, Tier 1; Rust, TS and LLVM
//! agree): the target element type picks the projection. `lista<K>` is the
//! keys, `lista<V>` is the values. When `K` and `V` are the same type the keys
//! win (both Rust and LLVM test the key type first). Order is unspecified, so
//! these pins compare as multisets.

use super::*;
use handroll::FxHashMap;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{Primitive, TypeId, TypeTable};

fn tabula_to_lista(
    key: Primitive,
    value: Primitive,
    target_element: Primitive,
    entries: Vec<(MapKey, Value)>,
) -> Result<Vec<Value>, RunnerError> {
    let mut type_table = TypeTable::new();
    let key = type_table.primitive(key);
    let value = type_table.primitive(value);
    let element = type_table.primitive(target_element);
    let map = type_table.map(key, value);
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
    let mut table = FxHashMap::default();
    table.extend(entries);
    let result = runner.convert_to_array(
        Value::Map(Rc::new(RefCell::new(table))),
        map,
        element,
        None,
        &[],
        Span::default(),
    )?;
    let Value::Array(items) = result else {
        panic!("expected lista, got {result:?}");
    };
    let mut items = items.borrow().clone();
    items.sort_by_key(|item| format!("{item:?}"));
    Ok(items)
}

fn int_to_text() -> Vec<(MapKey, Value)> {
    vec![
        (MapKey::UInt(5), Value::Str("x".into())),
        (MapKey::UInt(9), Value::Str("y".into())),
    ]
}

fn text_to_int() -> Vec<(MapKey, Value)> {
    vec![
        (MapKey::Str("a".into()), Value::Int(1)),
        (MapKey::Str("b".into()), Value::Int(2)),
    ]
}

#[test]
fn int_to_text_map_projects_keys_for_lista_int() {
    let got = tabula_to_lista(
        Primitive::Numerus,
        Primitive::Textus,
        Primitive::Numerus,
        int_to_text(),
    )
    .expect("tabula<int, text> to lista<int>");
    assert_eq!(got, vec![Value::UInt(5), Value::UInt(9)]);
}

#[test]
fn int_to_text_map_projects_values_for_lista_text() {
    let got = tabula_to_lista(
        Primitive::Numerus,
        Primitive::Textus,
        Primitive::Textus,
        int_to_text(),
    )
    .expect("tabula<int, text> to lista<text>");
    assert_eq!(got, vec![Value::Str("x".into()), Value::Str("y".into())]);
}

#[test]
fn text_to_int_map_projects_keys_for_lista_text() {
    let got = tabula_to_lista(
        Primitive::Textus,
        Primitive::Numerus,
        Primitive::Textus,
        text_to_int(),
    )
    .expect("tabula<text, int> to lista<text>");
    assert_eq!(got, vec![Value::Str("a".into()), Value::Str("b".into())]);
}

#[test]
fn text_to_int_map_projects_values_for_lista_int() {
    let got = tabula_to_lista(
        Primitive::Textus,
        Primitive::Numerus,
        Primitive::Numerus,
        text_to_int(),
    )
    .expect("tabula<text, int> to lista<int>");
    assert_eq!(got, vec![Value::Int(1), Value::Int(2)]);
}

#[test]
fn same_key_and_value_type_projects_keys() {
    let entries = vec![(MapKey::Str("k".into()), Value::Str("v".into()))];
    let got = tabula_to_lista(
        Primitive::Textus,
        Primitive::Textus,
        Primitive::Textus,
        entries,
    )
    .expect("tabula<text, text> to lista<text>");
    assert_eq!(got, vec![Value::Str("k".into())]);
}

#[test]
fn element_matching_neither_side_fails_closed() {
    let err = tabula_to_lista(
        Primitive::Numerus,
        Primitive::Textus,
        Primitive::Bivalens,
        int_to_text(),
    )
    .expect_err("lista<bool> from tabula<int, text> must not guess");
    assert_eq!(
        err.issue, "runner_valor_to_lista_conversion_failed",
        "{err:?}"
    );
}
