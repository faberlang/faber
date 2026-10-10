//! The runner prints `bivalens`, the null value and a tuple head through the
//! display-token table of the printing function (need, unit U3).
#![allow(clippy::default_trait_access)]

use super::*;
use crate::host::BufferHost;
use crate::value::{DisplayNames, FieldOrder, MapKey, TuplePart, TupleParts};
use radix_lexer::Interner;
use radix_mir::{
    MirBlock, MirConstant, MirDiagnosticKind, MirFunction, MirIntrinsic, MirProgram,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind,
    MirValidationContext,
};
use radix_types::{Primitive, TypeTable};
use std::{cell::RefCell, rc::Rc};

fn english() -> DisplayTokens {
    DisplayTokens {
        true_: "true".to_owned(),
        false_: "false".to_owned(),
        none: "none".to_owned(),
        tuple: "tuple".to_owned(),
    }
}

fn show(value: &Value, order: FieldOrder<'_>) -> String {
    show_with(value, &Interner::new(), order)
}

fn show_with(value: &Value, interner: &Interner, order: FieldOrder<'_>) -> String {
    value
        .display(interner, order, Span::default())
        .expect("display")
}

fn empty_order<'a>(
    structs: &'a value::StructFieldOrder,
    variants: &'a value::StructFieldOrder,
) -> FieldOrder<'a> {
    FieldOrder::new(structs, variants)
}

fn nested_tuple_list() -> Value {
    let tuple = Value::Tuple(vec![Value::Bool(true), Value::Nil]);
    Value::Array(Rc::new(RefCell::new(vec![tuple])))
}

fn tuple_parts() -> TupleParts {
    TupleParts(vec![
        TuplePart {
            width: None,
            tuple: None,
        },
        TuplePart {
            width: None,
            tuple: None,
        },
    ])
}

#[test]
fn english_tokens_print_bool_and_none() {
    let (structs, variants) = Default::default();
    let tokens = english();
    let order = empty_order(&structs, &variants).with_tokens(&tokens);
    assert_eq!(show(&Value::Bool(true), order), "true");
    assert_eq!(show(&Value::Bool(false), order), "false");
    assert_eq!(show(&Value::Nil, order), "none");
}

#[test]
fn english_tokens_print_nested_list_with_tuple() {
    let (structs, variants) = Default::default();
    let tokens = english();
    let parts = tuple_parts();
    let order = empty_order(&structs, &variants)
        .with_tokens(&tokens)
        .with_tuple(Some(&parts));
    assert_eq!(show(&nested_tuple_list(), order), "[tuple [true, none]]");
}

#[test]
fn english_tokens_print_map_with_unit_key() {
    let (structs, variants) = Default::default();
    let tokens = english();
    let order = empty_order(&structs, &variants).with_tokens(&tokens);
    let mut entries = rustc_hash::FxHashMap::default();
    entries.insert(MapKey::Unit, Value::Bool(false));
    let map = Value::Map(Rc::new(RefCell::new(entries)));
    assert_eq!(show(&map, order), "{none: false}");
}

#[test]
fn english_tokens_print_class_with_bool_field() {
    let mut interner = Interner::new();
    let field = interner.intern("ok");
    let def = radix_hir::DefId(9);
    let mut structs = value::StructFieldOrder::default();
    structs.insert(def, vec![field]);
    let variants = Default::default();
    let mut names = DisplayNames::default();
    names.insert(def, "Flag".to_owned());
    let tokens = english();
    let fields = Rc::new(RefCell::new(
        [(field, Value::Bool(true))].into_iter().collect(),
    ));
    let value = Value::Struct(def, fields);
    let latin = DisplayTokens::latin();
    let base = FieldOrder {
        names: &names,
        ..FieldOrder::new(&structs, &variants)
    };
    assert_eq!(
        show_with(&value, &interner, base.with_tokens(&tokens)),
        "Flag { ok = true }"
    );
    assert_eq!(
        show_with(&value, &interner, base),
        show_with(&value, &interner, base.with_tokens(&latin))
    );
    assert_eq!(show_with(&value, &interner, base), "Flag { ok = verum }");
}

#[test]
fn default_latin_tokens_print_what_was_printed_before() {
    let (structs, variants) = Default::default();
    let parts = tuple_parts();
    let order = empty_order(&structs, &variants).with_tuple(Some(&parts));
    assert_eq!(show(&Value::Bool(true), order), "verum");
    assert_eq!(show(&Value::Bool(false), order), "falsum");
    assert_eq!(show(&Value::Nil, order), "nihil");
    assert_eq!(show(&nested_tuple_list(), order), "[iuncta [verum, nihil]]");
    let mut entries = rustc_hash::FxHashMap::default();
    entries.insert(MapKey::Unit, Value::Bool(false));
    let map = Value::Map(Rc::new(RefCell::new(entries)));
    assert_eq!(show(&map, order), "{nihil: falsum}");
}

fn print_bool_function(id: u32, types: &TypeTable) -> MirFunction {
    let void = radix_mir::MirType::semantic(types.primitive(Primitive::Vacuum));
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
                        args: vec![MirOperand::Constant(MirConstant::Bool(true))],
                        return_ty: void,
                    },
                },
                span: Span::default(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: void,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

/// Runs function 0 (prints `true`) with `english()` assigned to `assigned`.
fn run_print(assigned: Option<MirFunctionId>) -> Vec<String> {
    let types = TypeTable::new();
    let interner = Interner::new();
    let mut context = MirValidationContext::new(&types);
    if let Some(function) = assigned {
        context.display_tokens.assign(function, &english());
    }
    let mut host = BufferHost::default();
    let program = MirProgram {
        functions: vec![print_bool_function(0, &types)],
    };
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.run_until_depth(0);
    assert!(runner.errors.is_empty(), "{:?}", runner.errors);
    host.stdout_lines.clone()
}

#[test]
fn runner_prints_through_the_function_entry() {
    assert_eq!(run_print(Some(MirFunctionId(0))), ["true"]);
}

#[test]
fn runner_falls_back_to_latin_without_a_function_entry() {
    // No entry at all, and an entry for another function id: both fall back.
    assert_eq!(run_print(None), ["verum"]);
    assert_eq!(run_print(Some(MirFunctionId(7))), ["verum"]);
}

fn bool_key_map() -> Value {
    let mut entries = rustc_hash::FxHashMap::default();
    entries.insert(MapKey::Bool(true), Value::Int(1));
    Value::Map(Rc::new(RefCell::new(entries)))
}

#[test]
fn english_tokens_print_bool_map_key_as_true_false() {
    let (structs, variants) = Default::default();
    let tokens = english();
    let order = empty_order(&structs, &variants).with_tokens(&tokens);
    assert_eq!(show(&bool_key_map(), order), "{true: 1}");
    let mut entries = rustc_hash::FxHashMap::default();
    entries.insert(MapKey::Bool(false), Value::Int(2));
    let map = Value::Map(Rc::new(RefCell::new(entries)));
    assert_eq!(show(&map, order), "{false: 2}");
}

#[test]
fn latin_tokens_print_bool_map_key_as_verum_falsum() {
    let (structs, variants) = Default::default();
    let latin = DisplayTokens::latin();
    let order = empty_order(&structs, &variants).with_tokens(&latin);
    assert_eq!(show(&bool_key_map(), order), "{verum: 1}");
    let mut entries = rustc_hash::FxHashMap::default();
    entries.insert(MapKey::Bool(false), Value::Int(2));
    let map = Value::Map(Rc::new(RefCell::new(entries)));
    assert_eq!(show(&map, order), "{falsum: 2}");
    // A composite key's bool part prints through the tokens too.
    let mut entries = rustc_hash::FxHashMap::default();
    entries.insert(
        MapKey::Tuple(vec![MapKey::Bool(true), MapKey::UInt(3)]),
        Value::Int(1),
    );
    let map = Value::Map(Rc::new(RefCell::new(entries)));
    assert_eq!(show(&map, order), "{verum,3: 1}");
}

#[test]
fn bool_key_inside_a_list_of_maps_uses_the_function_tokens() {
    let (structs, variants) = Default::default();
    let list = Value::Array(Rc::new(RefCell::new(vec![bool_key_map()])));
    let tokens = english();
    let order = empty_order(&structs, &variants).with_tokens(&tokens);
    assert_eq!(show(&list, order), "[{true: 1}]");
    let latin = DisplayTokens::latin();
    let order = empty_order(&structs, &variants).with_tokens(&latin);
    assert_eq!(show(&list, order), "[{verum: 1}]");
}

fn set_of(items: Vec<MapKey>) -> Value {
    Value::Set(Rc::new(RefCell::new(items.into_iter().collect())))
}

#[test]
fn english_tokens_print_bool_set_elements_as_true_false() {
    let (structs, variants) = Default::default();
    let tokens = english();
    let order = empty_order(&structs, &variants).with_tokens(&tokens);
    assert_eq!(show(&set_of(vec![MapKey::Bool(true)]), order), "[true]");
    assert_eq!(show(&set_of(vec![MapKey::Bool(false)]), order), "[false]");
}

#[test]
fn latin_tokens_print_bool_set_elements_as_verum_falsum() {
    let (structs, variants) = Default::default();
    let latin = DisplayTokens::latin();
    let order = empty_order(&structs, &variants).with_tokens(&latin);
    assert_eq!(show(&set_of(vec![MapKey::Bool(true)]), order), "[verum]");
    assert_eq!(show(&set_of(vec![MapKey::Bool(false)]), order), "[falsum]");
}

#[test]
fn one_element_sets_print_in_list_shape() {
    let (structs, variants) = Default::default();
    let tokens = english();
    let order = empty_order(&structs, &variants).with_tokens(&tokens);
    assert_eq!(show(&set_of(vec![MapKey::UInt(3)]), order), "[3]");
    assert_eq!(
        show(&set_of(vec![MapKey::Str("x".to_owned())]), order),
        "[\"x\"]"
    );
    assert_eq!(show(&set_of(vec![]), order), "[]");
}

#[test]
fn three_element_set_prints_list_shape_with_the_same_multiset_under_two_seeds() {
    let (structs, variants) = Default::default();
    let tokens = english();
    let order = empty_order(&structs, &variants).with_tokens(&tokens);
    let items: FxHashSet<MapKey> = [3_u64, 7, 11].into_iter().map(MapKey::UInt).collect();
    let multiset = |seed: u64| {
        let mut keys: Vec<&MapKey> = items.iter().collect();
        crate::shuffle::permute(&mut keys, seed);
        let text = value::display_set_keys(&keys, &Interner::new(), order, Span::default(), None)
            .expect("display");
        assert!(text.starts_with('[') && text.ends_with(']'), "{text}");
        let mut parts: Vec<String> = text[1..text.len() - 1]
            .split(", ")
            .map(str::to_owned)
            .collect();
        parts.sort();
        parts
    };
    assert_eq!(multiset(1), multiset(2));
    assert_eq!(multiset(1), ["11", "3", "7"]);
    let printed = show(
        &set_of(vec![MapKey::UInt(3), MapKey::UInt(7), MapKey::UInt(11)]),
        order,
    );
    assert!(
        printed.starts_with('[') && printed.ends_with(']'),
        "{printed}"
    );
}
