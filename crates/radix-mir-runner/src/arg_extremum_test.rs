//! LSR-W0-SEMANTIC runner proofs for the ArgMax/ArgMin collection ops —
//! the four test-pinned contract items (wave-0-delivery.md §3):
//!
//! 1. first-index tie law — one law, also the parallel-combine law
//!    (partials combine as `(value desc, index asc)` pairs);
//! 2. empty source → `nihil` — no −1 sentinel path exists;
//! 3. NaN law pinned to `max from`'s IMPLEMENTED law (the `TensorMax`
//!    `f64::max` fold, maxNum skip semantics);
//! 4. argmin mirrors argmax exactly.
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(clippy::cast_precision_loss, clippy::float_cmp)]

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::host::BufferHost;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirRuntimeCall, MirType,
};
use radix_types::TypeTable;

fn host_function() -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(radix_types::TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn eval_arg_extremum(op: MirCollectionOp, data: Vec<Value>) -> Result<Value, RunnerError> {
    let mut host = BufferHost::default();
    let types = TypeTable::default();
    let interner = Interner::new();
    let context = radix_mir::MirValidationContext::new(&types);
    let mut program = radix_mir::MirProgram::new();
    program.functions.push(host_function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), radix_mir::MirBlockId(0));
    runner
        .frames
        .last_mut()
        .expect("arg-extremum frame")
        .values
        .insert(MirValueId(0), Value::Array(Rc::new(RefCell::new(data))));

    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(op),
            args: vec![MirOperand::Value(MirValueId(0))],
            return_ty: MirType::semantic(radix_types::TypeId(0)),
        },
        Span::default(),
    )
}

fn argmax(data: Vec<Value>) -> Value {
    eval_arg_extremum(MirCollectionOp::ArgMax, data).expect("argmax runtime call")
}

fn argmin(data: Vec<Value>) -> Value {
    eval_arg_extremum(MirCollectionOp::ArgMin, data).expect("argmin runtime call")
}

fn floats(values: &[f64]) -> Vec<Value> {
    values.iter().map(|value| Value::Float(*value)).collect()
}

fn as_f64(value: &Value) -> f64 {
    match value {
        Value::Float(value) => *value,
        Value::Int(value) => *value as f64,
        Value::UInt(value) => *value as f64,
        other => panic!("non-numeric element {other:?}"),
    }
}

/// f64 equality that also matches NaN to NaN (the fold pin needs it).
fn same_or_both_nan(left: f64, right: f64) -> bool {
    left == right || (left.is_nan() && right.is_nan())
}

// ---------------------------------------------------------------------------
// Contract 1 — first-index tie law, stated once; also the combine law
// ---------------------------------------------------------------------------

/// The sequential scan keeps the FIRST maximal index — and because the law is
/// `(value desc, index asc)` pairing, any split into partials followed by the
/// same combine produces the identical winner. Proving scan == combine here
/// is the pin that host scan and device tree-combine can never diverge.
#[test]
fn argmax_first_index_tie_law_is_the_parallel_combine_law() {
    let ties = floats(&[3.0, 1.0, 3.0, 2.0, 3.0]);
    assert_eq!(
        argmax(ties.clone()),
        Value::Int(0),
        "least index keeps ties"
    );

    // Partials over disjoint windows, combined by (value desc, index asc):
    // the combine is implemented in the test from the law alone, and must
    // reproduce the full-scan winner for every split point.
    let full = [3.0_f64, 1.0, 3.0, 2.0, 3.0];
    for split in 1..full.len() {
        let (left, right) = (floats(&full[..split]), floats(&full[split..]));
        let left_winner = match argmax(left.clone()) {
            Value::Int(index) => {
                let index = usize::try_from(index).expect("index");
                (as_f64(&left[index]), index)
            }
            other => panic!("partial argmax returned {other:?}"),
        };
        let right_winner = match argmax(right.clone()) {
            Value::Int(index) => {
                let index = usize::try_from(index).expect("index") + split;
                (full[index], index)
            }
            other => panic!("partial argmax returned {other:?}"),
        };
        // (value desc, index asc) combine: strictly greater value wins; on
        // equal values the lesser index wins.
        let combined = if right_winner.0 > left_winner.0
            || (right_winner.0 == left_winner.0 && right_winner.1 < left_winner.1)
        {
            right_winner
        } else {
            left_winner
        };
        assert_eq!(
            Value::Int(i64::try_from(combined.1).expect("index")),
            argmax(floats(&full)),
            "split at {split}: partial combine must equal the full scan"
        );
    }
}

// ---------------------------------------------------------------------------
// Contract 2 — empty source → nihil; no −1 sentinel path anywhere
// ---------------------------------------------------------------------------

#[test]
fn argmax_empty_source_is_nihil_never_a_minus_one_sentinel() {
    let empty = argmax(Vec::new());
    assert_eq!(empty, Value::Nil, "empty argmax is nihil");
    assert_ne!(empty, Value::Int(-1), "no −1 sentinel path exists");
    let mirrored = argmin(Vec::new());
    assert_eq!(mirrored, Value::Nil, "empty argmin is nihil");
    assert_ne!(mirrored, Value::Int(-1), "no −1 sentinel path exists");
}

// ---------------------------------------------------------------------------
// Contract 3 — NaN law pinned to `max from`'s implemented law
// ---------------------------------------------------------------------------

/// `max from` lowers to `TensorMax`, whose implemented law is the
/// left-to-right `f64::max` fold (maxNum skip: NaN loses to every number,
/// all-NaN stays NaN). The pin: the argmax winner's VALUE equals that fold's
/// result, and a NaN element never wins while any number participates.
#[test]
fn argmax_nan_law_matches_the_max_from_tensor_max_fold() {
    let cases: &[&[f64]] = &[
        &[1.0, f64::NAN, 2.0],
        &[f64::NAN, 1.0],
        &[f64::NAN, f64::NAN, f64::NAN],
        &[2.0, f64::NAN, 2.0],
        &[f64::NAN],
    ];
    for case in cases {
        let winner = match argmax(floats(case)) {
            Value::Int(index) => usize::try_from(index).expect("index"),
            Value::Nil => {
                assert!(case.is_empty());
                continue;
            }
            other => panic!("argmax returned {other:?}"),
        };
        // The implemented `max from` law: fold with f64::max.
        let folded = case.iter().copied().fold(f64::NAN, f64::max);
        assert!(
            same_or_both_nan(case[winner], folded),
            "argmax winner value must equal the max-from fold for {case:?}"
        );
        if case.iter().any(|value| !value.is_nan()) {
            assert!(
                !case[winner].is_nan(),
                "NaN never wins while a number participates: {case:?}"
            );
        } else {
            assert_eq!(winner, 0, "all-NaN source keeps index 0");
        }
    }
    // The exact displacement case: NaN leads, a number follows — maxNum skip.
    assert_eq!(argmax(floats(&[f64::NAN, 1.0])), Value::Int(1));
}

// ---------------------------------------------------------------------------
// Contract 4 — argmin mirrors argmax exactly
// ---------------------------------------------------------------------------

#[test]
fn argmin_mirrors_argmax_exactly() {
    // Least value, first index on ties.
    assert_eq!(argmin(floats(&[2.0, 1.0, 1.0, 3.0])), Value::Int(1));
    assert_eq!(argmin(floats(&[1.0, 1.0])), Value::Int(0));
    // Mirror of the NaN law against the implemented TensorMin fold
    // (`f64::min`, maxNum skip).
    for case in [
        &[1.0, f64::NAN, -2.0][..],
        &[f64::NAN, -1.0][..],
        &[f64::NAN, f64::NAN][..],
    ] {
        let winner = match argmin(floats(case)) {
            Value::Int(index) => usize::try_from(index).expect("index"),
            other => panic!("argmin returned {other:?}"),
        };
        let folded = case.iter().copied().fold(f64::NAN, f64::min);
        assert!(
            same_or_both_nan(case[winner], folded),
            "argmin winner value must equal the min-from fold for {case:?}"
        );
    }
    assert_eq!(argmin(floats(&[f64::NAN, -1.0])), Value::Int(1));
    assert_eq!(argmin(floats(&[f64::NAN, f64::NAN])), Value::Int(0));
    // Integer elements compare exactly (never through an f64 round-trip).
    let ints = vec![Value::Int(4), Value::Int(-4), Value::Int(-4), Value::Int(9)];
    assert_eq!(argmax(ints.clone()), Value::Int(3));
    assert_eq!(argmin(ints), Value::Int(1));
}

// ---------------------------------------------------------------------------
// rn-amx — `lista<inf>`: exact by value, first index on ties
// ---------------------------------------------------------------------------

fn big(text: &str) -> Value {
    crate::inf::from_magnus(handroll::Magnus::parse_decimal(text).expect("decimal"))
}

/// A `lista<inf>` mixes the canonical carriers (`Int`, `UInt`, `Magnus`); the
/// winner is the exact greatest element, never an `f64` approximation, and
/// the first index wins ties.
#[test]
fn argmax_argmin_over_inf_compare_exact_values() {
    // 2^70 and 2^70 + 1 collapse to the same f64; the exact compare must not.
    let low = big("1180591620717411303424");
    let high = big("1180591620717411303425");
    assert!(matches!(low, Value::Magnus(_)));
    let xs = vec![
        Value::Int(3),
        low.clone(),
        high.clone(),
        Value::Int(-5),
        high.clone(),
        big("-1180591620717411303425"),
    ];
    assert_eq!(
        argmax(xs.clone()),
        Value::Int(2),
        "exact max, first on ties"
    );
    assert_eq!(argmin(xs), Value::Int(5), "exact min is the big negative");

    // Mixed `Int` / `UInt` carriers past i64::MAX compare by value.
    let mixed = vec![Value::Int(-1), Value::UInt(u64::MAX), Value::Int(i64::MAX)];
    assert_eq!(argmax(mixed.clone()), Value::Int(1));
    assert_eq!(argmin(mixed), Value::Int(0));

    // First index on ties, both twins.
    let ties = vec![low.clone(), Value::Int(1), low];
    assert_eq!(argmax(ties.clone()), Value::Int(0));
    assert_eq!(argmin(ties), Value::Int(1));
}
