//! Store-boundary pin for the same-type convert path (need caveat 2).
//!
//! The reverted runner identity-conversion skip (lane
//! `fix/runner-perf-20261016`, revert `1ff031529c`) lived in
//! `eval_runtime_convert` and would have bypassed the range check on
//! same-type numeric converts. What keeps a store-carrying same-type convert
//! checked is that `eval_convert` runs `eval_implicit_store` before the flavor
//! dispatch — and no existing test builds a `MirConversion` with
//! `store: Some`, so that ordering was invisible to the suite. This test pins
//! the implicit-store check on the same-type convert path: it goes red if a
//! future fast path hoists a same-type skip above the store check in
//! `eval_convert`.

use super::*;
use crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP;
use crate::value::Value;
use radix_hir::{HirStoreKind, HirStoreSite};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConversion, MirConversionFlavor, MirFunction, MirFunctionId, MirOperand,
    MirProgram, MirType, MirValidationContext, MirValueId,
};
use radix_types::{NumericWidth, Primitive, TypeId, TypeTable};

/// Canonical one-function program shape: no params, locals, or blocks.
fn make_function() -> MirFunction {
    MirFunction {
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
    }
}

/// The exact lowered shape of a checked integer store
/// (`integer_store_convert`, `crates/radix-mir-lower/src/lower.rs`): the
/// source and target type ids are the SAME type, so the conversion body is
/// nominally an identity and only the store can reject the value.
fn same_type_store_convert(slot_ty: TypeId) -> MirConversion {
    MirConversion {
        store: Some(HirStoreSite {
            kind: HirStoreKind::Declaration,
            slot: None,
            inferred_from: None,
            inferred_text: None,
        }),
        flavor: MirConversionFlavor::Runtime,
        source_ty: MirType::semantic(slot_ty),
        target_ty: MirType::semantic(slot_ty),
        params: Vec::new(),
        recovery: None,
        struct_defaults: Vec::new(),
    }
}

#[test]
fn same_type_store_convert_traps_the_exact_i64_successor() {
    let mut types = TypeTable::new();
    let i64_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(make_function());

    let mut host = crate::host::BufferHost::default();
    let interner = Interner::new();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);

    let fn0 = program.functions.first().expect("the canonical function");
    runner.push_frame(fn0.id, MirBlockId(0));

    // 2^63 = i64::MAX + 1: the exact successor of the i64 slot boundary. It is
    // inside the ruling-17 exact carrier (`u64`) but outside the i64 slot.
    runner
        .frames
        .last_mut()
        .expect("a live frame")
        .values
        .insert(MirValueId(0), Value::UInt(9_223_372_036_854_775_808));
    runner
        .frames
        .last_mut()
        .expect("a live frame")
        .values
        .insert(MirValueId(1), Value::Int(i64::MAX));

    let conversion = same_type_store_convert(i64_ty);

    let trap = runner
        .eval_convert(
            &conversion,
            &[MirOperand::Value(MirValueId(0))],
            Span::default(),
        )
        .expect_err("the same-type store must still check the i64 slot");
    assert_eq!(trap.issue, RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
    assert_eq!(trap.arg("value"), Some("9223372036854775808"));
    assert_eq!(trap.arg("type"), Some("i64"));

    // The in-range neighbour through the same conversion: the check sits at
    // the exact boundary, it is not a blanket rejection.
    let stored = runner
        .eval_convert(
            &conversion,
            &[MirOperand::Value(MirValueId(1))],
            Span::default(),
        )
        .expect("i64::MAX fits the i64 slot");
    assert_eq!(stored, Value::Int(i64::MAX));
}
