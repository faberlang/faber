//! Ranged store policies on the runner: a store into a
//! `policy<W, lo‥hi>` cell applies the policy to the range, not to the width.
//! `saturating` clamps into `[lo, hi]`, `wrapping` reduces modulo
//! `hi - lo + 1` offset by `lo`, `trapping` traps outside it, and the trap names
//! the range. Arithmetic is exact, so the store sees the unreduced value.

use super::*;
use crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP;
use crate::value::Value;
use radix_hir::{HirStoreKind, HirStoreSite};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConversion, MirConversionFlavor, MirFunction, MirFunctionId, MirOperand,
    MirProgram, MirType, MirValidationContext, MirValueId,
};
use radix_types::{IntPolicy, NumericWidth, TypeId, TypeTable};

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

fn store_convert(slot_ty: TypeId) -> MirConversion {
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

/// Store each `(input, expected)` pair into the slot and compare.
fn stores(
    policy: IntPolicy,
    width: NumericWidth,
    (lo, hi): (i128, i128),
    cases: &[(Value, Result<Value, &str>)],
) {
    let mut types = TypeTable::new();
    let slot = types.ranged_int(policy, width, lo, hi);
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(make_function());
    let mut host = crate::host::BufferHost::default();
    let interner = Interner::new();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let function = program.functions.first().expect("the canonical function");
    runner.push_frame(function.id, MirBlockId(0));
    let conversion = store_convert(slot);
    for (index, (input, expected)) in cases.iter().enumerate() {
        runner
            .frames
            .last_mut()
            .expect("a live frame")
            .values
            .insert(MirValueId(index as u32), input.clone());
        let result = runner.eval_convert(
            &conversion,
            &[MirOperand::Value(MirValueId(index as u32))],
            Span::default(),
        );
        match (result, expected) {
            (Ok(stored), Ok(want)) => assert_eq!(&stored, want, "input {input:?}"),
            (Err(trap), Err(type_text)) => {
                assert_eq!(trap.issue, RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
                assert_eq!(trap.arg("type"), Some(*type_text), "input {input:?}");
            }
            (other, want) => panic!("input {input:?}: got {other:?}, wanted {want:?}"),
        }
    }
}

#[test]
fn saturating_range_clamps_into_the_range_not_the_width() {
    stores(
        IntPolicy::Saturating,
        NumericWidth::U32,
        (0, 7),
        &[
            (Value::Int(20), Ok(Value::Int(7))),
            (Value::Int(7), Ok(Value::Int(7))),
            (Value::Int(3), Ok(Value::Int(3))),
            (Value::Int(0), Ok(Value::Int(0))),
            (Value::Int(-5), Ok(Value::Int(0))),
        ],
    );
}

#[test]
fn saturating_signed_range_clamps_at_both_signed_ends() {
    stores(
        IntPolicy::Saturating,
        NumericWidth::I32,
        (-2, 2),
        &[
            (Value::Int(-9), Ok(Value::Int(-2))),
            (Value::Int(9), Ok(Value::Int(2))),
            (Value::Int(-1), Ok(Value::Int(-1))),
        ],
    );
}

#[test]
fn wrapping_range_reduces_modulo_the_range_length_offset_by_the_low_end() {
    stores(
        IntPolicy::Wrapping,
        NumericWidth::U32,
        (0, 4),
        &[
            (Value::Int(12), Ok(Value::Int(2))),
            (Value::Int(5), Ok(Value::Int(0))),
            (Value::Int(-1), Ok(Value::Int(4))),
            (Value::Int(4), Ok(Value::Int(4))),
        ],
    );
    // A signed range with a negative low end: `-2‥3` holds -2 through 2.
    stores(
        IntPolicy::Wrapping,
        NumericWidth::I32,
        (-2, 2),
        &[
            (Value::Int(3), Ok(Value::Int(-2))),
            (Value::Int(100), Ok(Value::Int(0))),
            (Value::Int(-3), Ok(Value::Int(2))),
        ],
    );
}

#[test]
fn wrapping_range_that_does_not_start_at_zero_offsets_by_the_low_end() {
    stores(
        IntPolicy::Wrapping,
        NumericWidth::U8,
        (10, 12),
        &[
            (Value::Int(13), Ok(Value::Int(10))),
            (Value::Int(9), Ok(Value::Int(12))),
            (Value::Int(11), Ok(Value::Int(11))),
        ],
    );
}

#[test]
fn trapping_range_traps_outside_it_and_names_the_range() {
    stores(
        IntPolicy::Trapping,
        NumericWidth::I32,
        (-2, 2),
        &[
            (Value::Int(2), Ok(Value::Int(2))),
            (Value::Int(-2), Ok(Value::Int(-2))),
            (Value::Int(3), Err("trapping<i32, -2‥3>")),
            (Value::Int(-3), Err("trapping<i32, -2‥3>")),
        ],
    );
}

#[test]
fn u64_ranges_keep_the_unsigned_carrier() {
    stores(
        IntPolicy::Saturating,
        NumericWidth::U64,
        (0, 9),
        &[
            (Value::UInt(u64::MAX), Ok(Value::UInt(9))),
            (Value::UInt(4), Ok(Value::UInt(4))),
        ],
    );
}

#[test]
fn an_inf_beyond_the_carriers_follows_the_range_policy() {
    use handroll::Magnus;
    let beyond = Value::Magnus(Rc::new(Magnus::from_i128(i128::from(u64::MAX) + 100)));
    stores(
        IntPolicy::Saturating,
        NumericWidth::U32,
        (0, 7),
        &[(beyond.clone(), Ok(Value::Int(7)))],
    );
    stores(
        IntPolicy::Wrapping,
        NumericWidth::U32,
        (0, 6),
        // 2^64 + 99 mod 7 = (2^64 mod 7) + (99 mod 7) = 2 + 1 = 3 (mod 7)
        &[(beyond.clone(), Ok(Value::Int(3)))],
    );
    stores(
        IntPolicy::Trapping,
        NumericWidth::U32,
        (0, 7),
        &[(beyond, Err("trapping<u32, 0‥8>"))],
    );
}
