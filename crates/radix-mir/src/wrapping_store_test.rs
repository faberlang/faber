//! Wrapping-store region analysis proofs.
//!
//! The G03 fixture mirrors the real MIR lowering of
//! `corpus-simulations/swarm-decimal/repros/g03-wrapping-xor-then-multiply.fab`
//! (`(z ⊻ (z ⇒ 30)) * m` stored into `wrapping<u64>`): the shift, xor and
//! multiply temporaries carry the range-rule `numerus<u64>` type, and the
//! store is a `Convert` node with a store site into the word cell. Every
//! counterexample proves one conservative stop.

use super::*;
use crate::{
    MirBlock, MirConstant, MirFunctionId, MirLocal, MirLocalId, MirStatement, MirTemp,
    MirTerminator, MirValueId,
};
use radix_hir::{HirStoreKind, HirStoreSite};
use radix_lexer::{Span, Symbol};
use radix_types::{IntPolicy, NumericWidth, Primitive, Type, TypeId, TypeTable};

/// The interned types the fixtures use.
struct Tys {
    /// `wrapping<u64>` — the plain word cell.
    word: TypeId,
    /// `numerus<u64>` — the range-rule type of G03's intermediates.
    exact: TypeId,
    /// Bare `numerus`.
    plain: TypeId,
    /// `wrapping<u64, 0‥2^64-1>` — a ranged wrapping cell.
    ranged: TypeId,
    /// `saturating<u64>`.
    saturating: TypeId,
    /// `wrapping<inf>` — no width to reduce at.
    word_inf: TypeId,
}

fn types() -> (TypeTable, Tys) {
    let mut types = TypeTable::new();
    let tys = Tys {
        word: types.intern(Type::ModularWord(NumericWidth::U64)),
        exact: types.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::U64)),
        plain: types.primitive(Primitive::Numerus),
        ranged: types.intern(Type::RangedInt {
            policy: IntPolicy::Wrapping,
            width: NumericWidth::U64,
            lo: 0,
            hi: i128::from(u64::MAX),
        }),
        saturating: types.intern(Type::SaturatingWord(NumericWidth::U64)),
        word_inf: types.intern(Type::ModularWord(NumericWidth::Inf)),
    };
    (types, tys)
}

fn value(id: u32, kind: MirValueKind, ty: TypeId) -> MirValue {
    MirValue {
        id: MirValueId(id),
        kind,
        ty: MirType::semantic(ty),
        span: Span::default(),
    }
}

fn temp(id: u32) -> MirTempId {
    MirTempId(id)
}

fn local(id: u32) -> MirPlace {
    MirPlace::local(MirLocalId(id))
}

fn place_temp(id: u32) -> MirPlace {
    MirPlace::temp(temp(id))
}

/// `t<n> = <kind>` — an arithmetic temporary's defining statement.
fn temp_assign(id: u32, kind: MirValueKind, ty: TypeId) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: place_temp(id),
            value: value(id, kind, ty),
        },
        span: Span::default(),
    }
}

/// `l<n> = <operand>` — a plain slot assignment.
fn local_assign(id: u32, operand: MirOperand, ty: TypeId) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: local(id),
            value: value(id + 100, MirValueKind::Operand(operand), ty),
        },
        span: Span::default(),
    }
}

/// The store-conversion node: `t<dst> = convert store <source> -> <target>`.
fn convert_store(dst: u32, arg: MirOperand, source: TypeId, target: TypeId) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(place_temp(dst)),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Convert(crate::MirConversion {
                    store: Some(HirStoreSite {
                        kind: HirStoreKind::Declaration,
                        slot: None,
                        inferred_from: None,
                        inferred_text: None,
                    }),
                    flavor: crate::MirConversionFlavor::Runtime,
                    source_ty: MirType::semantic(source),
                    target_ty: MirType::semantic(target),
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                args: vec![arg],
                return_ty: MirType::semantic(target),
            },
        },
        span: Span::default(),
    }
}

/// `nota(<operand>)` — the diagnostic observation used by the shared-temp
/// counterexamples.
fn nota(operand: MirOperand) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Diagnostic(crate::MirDiagnosticKind::Nota),
                args: vec![operand],
                return_ty: MirType::semantic(TypeId(0)),
            },
        },
        span: Span::default(),
    }
}

fn binary(op: MirBinOp, lhs: MirOperand, rhs: MirOperand) -> MirValueKind {
    MirValueKind::Binary { op, lhs, rhs }
}

fn block(id: u32, statements: Vec<MirStatement>, terminator: MirTerminatorKind) -> MirBlock {
    MirBlock {
        id: MirBlockId(id),
        statements,
        terminator: MirTerminator {
            kind: terminator,
            span: Span::default(),
        },
        span: Span::default(),
    }
}

/// A function with the given word-typed locals and exact-typed temps.
fn function(
    locals: &[(u32, TypeId)],
    temps: &[(u32, TypeId)],
    blocks: Vec<MirBlock>,
) -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: locals
            .iter()
            .map(|(id, ty)| MirLocal {
                id: MirLocalId(*id),
                name: None,
                ty: MirType::semantic(*ty),
                mutable: false,
                span: Span::default(),
            })
            .collect(),
        temps: temps
            .iter()
            .map(|(id, ty)| MirTemp {
                id: MirTempId(*id),
                ty: MirType::semantic(*ty),
                span: Span::default(),
            })
            .collect(),
        blocks,
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

/// The G03 locals and temps: `_0`/`_1`/`_2` are `wrapping<u64>`, `%0` the
/// shift, `%1` the xor, `%2` the multiply, `%3` the store conversion.
fn g03_signature(tys: &Tys) -> (Vec<(u32, TypeId)>, Vec<(u32, TypeId)>) {
    (
        vec![(0, tys.word), (1, tys.word), (2, tys.word)],
        vec![
            (0, tys.exact),
            (1, tys.exact),
            (2, tys.exact),
            (3, tys.word),
        ],
    )
}

/// The G03 chain: `_0`/`_1` are `wrapping<u64>` locals, `%0 = _0 >> 30`,
/// `%1 = _0 ^ %0`, `%2 = %1 * _1`, `%3 = convert store(%2)`, `_2 = %3`.
fn g03_statements(tys: &Tys) -> Vec<MirStatement> {
    vec![
        local_assign(
            0,
            MirOperand::Constant(MirConstant::UInt(8_027_708_234_668_681_072)),
            tys.word,
        ),
        local_assign(
            1,
            MirOperand::Constant(MirConstant::UInt(13_787_848_793_156_543_929)),
            tys.word,
        ),
        temp_assign(
            0,
            binary(
                MirBinOp::Shr,
                MirOperand::Place(local(0)),
                MirOperand::Constant(MirConstant::UInt(30)),
            ),
            tys.exact,
        ),
        temp_assign(
            1,
            binary(
                MirBinOp::BitXor,
                MirOperand::Place(local(0)),
                MirOperand::Temp(temp(0)),
            ),
            tys.exact,
        ),
        temp_assign(
            2,
            binary(
                MirBinOp::Mul,
                MirOperand::Temp(temp(1)),
                MirOperand::Place(local(1)),
            ),
            tys.exact,
        ),
        convert_store(3, MirOperand::Temp(temp(2)), tys.exact, tys.word),
        local_assign(2, MirOperand::Temp(temp(3)), tys.word),
        nota(MirOperand::Place(local(2))),
    ]
}

fn g03_function(tys: &Tys) -> MirFunction {
    let (locals, temps) = g03_signature(tys);
    function(
        &locals,
        &temps,
        vec![block(
            0,
            g03_statements(&tys),
            MirTerminatorKind::Return(None),
        )],
    )
}

#[test]
fn g03_chain_reduces_under_the_wrapping_store() {
    let (types, tys) = types();
    let regions = wrapping_store_regions(&g03_function(&tys), &types);
    assert_eq!(regions.len(), 1, "the G03 store yields one region");
    let region = &regions[0];
    assert_eq!(region.block, MirBlockId(0));
    assert_eq!(region.statement, 5, "rooted at the store-conversion node");
    assert_eq!(region.site, WrappingStoreSite::Conversion);
    assert_eq!(region.width, NumericWidth::U64);
    // The multiply and the xor commute with reduction mod 2^64 and are
    // claimed; the shift is a floor division by 2^30 and stays exact.
    assert_eq!(region.temps, vec![temp(2), temp(1)]);
    assert_eq!(
        wrapping_reduce_width(&regions, temp(2)),
        Some(NumericWidth::U64)
    );
    assert_eq!(
        wrapping_reduce_width(&regions, temp(1)),
        Some(NumericWidth::U64)
    );
    assert_eq!(wrapping_reduce_width(&regions, temp(0)), None);
    assert_eq!(wrapping_reduce_width(&regions, temp(3)), None);
}

#[test]
fn shared_temp_is_refused_but_the_rest_reduces() {
    // The xor temp is also printed: its edge into the multiply is no longer
    // its only reader, so the xor stays exact. The multiply itself still
    // reduces under the store.
    let (types, tys) = types();
    let mut statements = g03_statements(&tys);
    statements.insert(5, nota(MirOperand::Temp(temp(1))));
    let (locals, temps) = g03_signature(&tys);
    let f = function(
        &locals,
        &temps,
        vec![block(0, statements, MirTerminatorKind::Return(None))],
    );
    let regions = wrapping_store_regions(&f, &types);
    assert_eq!(regions.len(), 1);
    assert_eq!(
        regions[0].temps,
        vec![temp(2)],
        "a shared intermediate stays exact; the multiply still reduces"
    );
}

#[test]
fn shared_root_refuses_the_region() {
    // The store's own root is shared with a diagnostic: nothing reduces.
    let (types, tys) = types();
    let mut statements = g03_statements(&tys);
    let store = statements.remove(5);
    statements.insert(5, nota(MirOperand::Temp(temp(2))));
    statements.insert(6, store);
    let (locals, temps) = g03_signature(&tys);
    let f = function(
        &locals,
        &temps,
        vec![block(0, statements, MirTerminatorKind::Return(None))],
    );
    let regions = wrapping_store_regions(&f, &types);
    assert!(
        regions.is_empty(),
        "a multiply with a second reader may not reduce"
    );
}

#[test]
fn terminator_observation_is_refused() {
    // The block's return terminator reads the root: an observation boundary
    // outside the statements, proving terminators are scanned too.
    let (types, tys) = types();
    let (locals, temps) = g03_signature(&tys);
    let f = function(
        &locals,
        &temps,
        vec![block(
            0,
            g03_statements(&tys),
            MirTerminatorKind::Return(Some(MirOperand::Temp(temp(2)))),
        )],
    );
    let regions = wrapping_store_regions(&f, &types);
    assert!(
        regions.is_empty(),
        "a root observed by a terminator may not reduce"
    );
}

/// The `z * m` multiply used by the refusal fixtures.
fn mul_temp(tys: &Tys) -> MirStatement {
    temp_assign(
        2,
        binary(
            MirBinOp::Mul,
            MirOperand::Place(local(0)),
            MirOperand::Place(local(0)),
        ),
        tys.exact,
    )
}

#[test]
fn projected_write_is_refused() {
    // A store conversion whose destination carries a projection is a
    // projected write, not a whole-cell landing.
    let (types, tys) = types();
    let mut projected_place = place_temp(3);
    projected_place
        .projections
        .push(MirProjection::Field(Symbol(7)));
    let projected = MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(projected_place),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Convert(crate::MirConversion {
                    store: Some(HirStoreSite {
                        kind: HirStoreKind::Field,
                        slot: None,
                        inferred_from: None,
                        inferred_text: None,
                    }),
                    flavor: crate::MirConversionFlavor::Runtime,
                    source_ty: MirType::semantic(tys.exact),
                    target_ty: MirType::semantic(tys.word),
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                args: vec![MirOperand::Temp(temp(2))],
                return_ty: MirType::semantic(tys.word),
            },
        },
        span: Span::default(),
    };
    let mul = mul_temp(&tys);
    let f = function(
        &[(0, tys.word)],
        &[(2, tys.exact), (3, tys.word)],
        vec![block(
            0,
            vec![mul, projected],
            MirTerminatorKind::Return(None),
        )],
    );
    assert!(
        wrapping_store_regions(&f, &types).is_empty(),
        "a projected store conversion is not a wrapping-store region"
    );

    // A plain assignment through an index projection is refused the same
    // way, even though the local's own slot is a word.
    let indexed = MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace {
                base: MirPlaceBase::Local(MirLocalId(0)),
                projections: vec![MirProjection::Index(MirOperand::Constant(
                    MirConstant::Int(0),
                ))],
            },
            value: value(
                9,
                MirValueKind::Operand(MirOperand::Temp(temp(2))),
                tys.word,
            ),
        },
        span: Span::default(),
    };
    let f = function(
        &[(0, tys.word)],
        &[(2, tys.exact)],
        vec![block(
            0,
            vec![mul_temp(&tys), indexed],
            MirTerminatorKind::Return(None),
        )],
    );
    assert!(
        wrapping_store_regions(&f, &types).is_empty(),
        "a projected slot assignment is not a wrapping-store region"
    );
}

#[test]
fn cross_block_chain_is_refused() {
    // The multiply is defined in bb0, the store sits in bb1: unsupported
    // control flow.
    let (types, tys) = types();
    let mul = mul_temp(&tys);
    let f = function(
        &[(0, tys.word), (2, tys.word)],
        &[(2, tys.exact), (3, tys.word)],
        vec![
            block(0, vec![mul], MirTerminatorKind::Goto(MirBlockId(1))),
            block(
                1,
                vec![
                    convert_store(3, MirOperand::Temp(temp(2)), tys.exact, tys.word),
                    local_assign(2, MirOperand::Temp(temp(3)), tys.word),
                ],
                MirTerminatorKind::Return(None),
            ),
        ],
    );
    assert!(
        wrapping_store_regions(&f, &types).is_empty(),
        "a chain crossing a block edge may not reduce"
    );
}

#[test]
fn trycall_redefinition_is_refused() {
    // The multiply is defined by its assignment but also written by a
    // `TryCall` destination on the block's exit edge: two writes, no
    // single definition, so nothing reduces.
    let (types, tys) = types();
    let statements = vec![
        mul_temp(&tys),
        convert_store(3, MirOperand::Temp(temp(2)), tys.exact, tys.word),
    ];
    let try_write = MirTerminatorKind::TryCall {
        destination: Some(place_temp(2)),
        callee: crate::MirCallee::Function(crate::MirFunctionId(1)),
        args: Vec::new(),
        ok_block: MirBlockId(1),
        error_place: local(0),
        error_block: MirBlockId(1),
    };
    let f = function(
        &[(0, tys.word)],
        &[(2, tys.exact), (3, tys.word)],
        vec![
            block(0, statements, try_write),
            block(1, Vec::new(), MirTerminatorKind::Return(None)),
        ],
    );
    assert!(
        wrapping_store_regions(&f, &types).is_empty(),
        "a temp written by a try-call edge is not single-definition"
    );
}

#[test]
fn plain_slot_assign_store_is_recognized() {
    // `_1 = %0` where the slot is a word and `%0` is an exact-typed
    // multiply: the plain assignment itself is the wrapping store.
    let (types, tys) = types();
    let statements = vec![
        temp_assign(
            0,
            binary(
                MirBinOp::Mul,
                MirOperand::Place(local(0)),
                MirOperand::Place(local(0)),
            ),
            tys.exact,
        ),
        local_assign(1, MirOperand::Temp(temp(0)), tys.word),
    ];
    let f = function(
        &[(0, tys.word), (1, tys.word)],
        &[(0, tys.exact)],
        vec![block(0, statements, MirTerminatorKind::Return(None))],
    );
    let regions = wrapping_store_regions(&f, &types);
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].site, WrappingStoreSite::SlotAssign);
    assert_eq!(regions[0].statement, 1);
    assert_eq!(regions[0].width, NumericWidth::U64);
    assert_eq!(regions[0].temps, vec![temp(0)]);
}

#[test]
fn word_typed_chain_yields_no_region() {
    // The `z * z` shape: the multiply's own type is the word, so its
    // operation already reduces at the word and there is nothing to claim.
    let (types, tys) = types();
    let statements = vec![
        temp_assign(
            0,
            binary(
                MirBinOp::Mul,
                MirOperand::Place(local(0)),
                MirOperand::Place(local(0)),
            ),
            tys.word,
        ),
        local_assign(1, MirOperand::Temp(temp(0)), tys.word),
    ];
    let f = function(
        &[(0, tys.word), (1, tys.word)],
        &[(0, tys.word)],
        vec![block(0, statements, MirTerminatorKind::Return(None))],
    );
    assert!(
        wrapping_store_regions(&f, &types).is_empty(),
        "a word-typed top operation needs no region"
    );
}

#[test]
fn shift_count_stays_exact() {
    // `%1 = %0 << %2` into the store: the receiver joins the chain, the
    // count does not — an exact count beyond the word shifts differently.
    let (types, tys) = types();
    let statements = vec![
        temp_assign(
            0,
            binary(
                MirBinOp::Add,
                MirOperand::Place(local(0)),
                MirOperand::Place(local(0)),
            ),
            tys.exact,
        ),
        temp_assign(
            2,
            binary(
                MirBinOp::Add,
                MirOperand::Place(local(0)),
                MirOperand::Place(local(0)),
            ),
            tys.exact,
        ),
        temp_assign(
            1,
            binary(
                MirBinOp::Shl,
                MirOperand::Temp(temp(0)),
                MirOperand::Temp(temp(2)),
            ),
            tys.exact,
        ),
        convert_store(3, MirOperand::Temp(temp(1)), tys.exact, tys.word),
    ];
    let f = function(
        &[(0, tys.word)],
        &[
            (0, tys.exact),
            (1, tys.exact),
            (2, tys.exact),
            (3, tys.word),
        ],
        vec![block(0, statements, MirTerminatorKind::Return(None))],
    );
    let regions = wrapping_store_regions(&f, &types);
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].temps, vec![temp(1), temp(0)]);
    assert_eq!(
        wrapping_reduce_width(&regions, temp(2)),
        None,
        "the shift count never joins a region"
    );
}

#[test]
fn floor_division_and_shift_are_terminal() {
    // `(a / d) * m` into the word: the division result feeds the multiply
    // exactly; only the multiply reduces. The same holds for a shift.
    for op in [MirBinOp::Div, MirBinOp::Mod, MirBinOp::Shr] {
        let (types, tys) = types();
        let statements = vec![
            temp_assign(
                0,
                binary(op, MirOperand::Place(local(0)), MirOperand::Place(local(1))),
                tys.exact,
            ),
            temp_assign(
                2,
                binary(
                    MirBinOp::Mul,
                    MirOperand::Temp(temp(0)),
                    MirOperand::Place(local(1)),
                ),
                tys.exact,
            ),
            convert_store(3, MirOperand::Temp(temp(2)), tys.exact, tys.word),
        ];
        let f = function(
            &[(0, tys.word), (1, tys.word)],
            &[(0, tys.exact), (2, tys.exact), (3, tys.word)],
            vec![block(0, statements, MirTerminatorKind::Return(None))],
        );
        let regions = wrapping_store_regions(&f, &types);
        assert_eq!(regions.len(), 1, "{op:?} chain still stores one region");
        assert_eq!(
            regions[0].temps,
            vec![temp(2)],
            "{op:?} does not commute with reduction"
        );
    }
}

#[test]
fn non_word_destinations_are_refused() {
    // A ranged wrapping cell reduces modulo its range, a saturating word
    // clamps, and `wrapping<inf>` has no width: none is a region
    // destination.
    let (types, tys) = types();
    for target in [tys.ranged, tys.saturating, tys.word_inf] {
        let statements = vec![
            temp_assign(
                0,
                binary(
                    MirBinOp::Mul,
                    MirOperand::Place(local(0)),
                    MirOperand::Place(local(0)),
                ),
                tys.exact,
            ),
            convert_store(1, MirOperand::Temp(temp(0)), tys.exact, target),
        ];
        let f = function(
            &[(0, target)],
            &[(0, tys.exact), (1, target)],
            vec![block(0, statements, MirTerminatorKind::Return(None))],
        );
        assert!(
            wrapping_store_regions(&f, &types).is_empty(),
            "target {target:?} is not a plain word destination"
        );
    }
}

#[test]
fn two_stores_yield_two_regions() {
    let (types, tys) = types();
    let statements = vec![
        temp_assign(
            0,
            binary(
                MirBinOp::Mul,
                MirOperand::Place(local(0)),
                MirOperand::Place(local(0)),
            ),
            tys.exact,
        ),
        convert_store(1, MirOperand::Temp(temp(0)), tys.exact, tys.word),
        temp_assign(
            2,
            binary(
                MirBinOp::Add,
                MirOperand::Place(local(0)),
                MirOperand::Place(local(0)),
            ),
            tys.exact,
        ),
        convert_store(3, MirOperand::Temp(temp(2)), tys.exact, tys.word),
    ];
    let f = function(
        &[(0, tys.word)],
        &[(0, tys.exact), (1, tys.word), (2, tys.exact), (3, tys.word)],
        vec![block(0, statements, MirTerminatorKind::Return(None))],
    );
    let regions = wrapping_store_regions(&f, &types);
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].statement, 1);
    assert_eq!(regions[0].temps, vec![temp(0)]);
    assert_eq!(regions[1].statement, 3);
    assert_eq!(regions[1].temps, vec![temp(2)]);
}

#[test]
fn constant_and_copy_roots_yield_no_region() {
    let (types, tys) = types();
    let constant_root = vec![convert_store(
        0,
        MirOperand::Constant(MirConstant::UInt(1)),
        tys.exact,
        tys.word,
    )];
    let copy_root = vec![
        local_assign(0, MirOperand::Constant(MirConstant::UInt(1)), tys.exact),
        temp_assign(
            1,
            MirValueKind::Operand(MirOperand::Place(local(0))),
            tys.exact,
        ),
        convert_store(2, MirOperand::Temp(temp(1)), tys.exact, tys.word),
    ];
    for statements in [constant_root, copy_root] {
        let f = function(
            &[(0, tys.exact)],
            &[(0, tys.word), (1, tys.exact), (2, tys.word)],
            vec![block(0, statements, MirTerminatorKind::Return(None))],
        );
        assert!(
            wrapping_store_regions(&f, &types).is_empty(),
            "a store of a constant or a plain copy claims nothing"
        );
    }
}

#[test]
fn bare_numerus_temps_join() {
    // The unbounded bare `numerus` family reduces under the word the same
    // way: the homomorphism does not care which exact carrier the
    // intermediate was typed with.
    let (types, tys) = types();
    let statements = vec![
        temp_assign(
            0,
            binary(
                MirBinOp::Mul,
                MirOperand::Place(local(0)),
                MirOperand::Place(local(0)),
            ),
            tys.plain,
        ),
        convert_store(1, MirOperand::Temp(temp(0)), tys.plain, tys.word),
    ];
    let f = function(
        &[(0, tys.word)],
        &[(0, tys.plain), (1, tys.word)],
        vec![block(0, statements, MirTerminatorKind::Return(None))],
    );
    let regions = wrapping_store_regions(&f, &types);
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].temps, vec![temp(0)]);
}

#[test]
fn copy_temp_is_transparent() {
    // `%1 = %0` then `%2 = %1 * l0` into the store: the copy claims nothing
    // itself, but the multiply and the add behind the copy both reduce.
    let (types, tys) = types();
    let statements = vec![
        temp_assign(
            0,
            binary(
                MirBinOp::Add,
                MirOperand::Place(local(0)),
                MirOperand::Place(local(0)),
            ),
            tys.exact,
        ),
        temp_assign(
            1,
            MirValueKind::Operand(MirOperand::Temp(temp(0))),
            tys.exact,
        ),
        temp_assign(
            2,
            binary(
                MirBinOp::Mul,
                MirOperand::Temp(temp(1)),
                MirOperand::Place(local(0)),
            ),
            tys.exact,
        ),
        convert_store(3, MirOperand::Temp(temp(2)), tys.exact, tys.word),
    ];
    let f = function(
        &[(0, tys.word)],
        &[
            (0, tys.exact),
            (1, tys.exact),
            (2, tys.exact),
            (3, tys.word),
        ],
        vec![block(0, statements, MirTerminatorKind::Return(None))],
    );
    let regions = wrapping_store_regions(&f, &types);
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].temps, vec![temp(2), temp(0)]);
}
