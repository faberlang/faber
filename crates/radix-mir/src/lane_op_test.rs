//! The per-lane rule of a lifted register operator ([`crate::lane_op`]), seen
//! through the vector expander: `÷` converts integer lanes to the result float
//! and traps a zero divisor, `⤒`/`⤓` widen both lanes then pick one, and a
//! `checked` operator stores every lane under the element type.
// Fixture casts are small known-in-range numbers.
#![allow(clippy::cast_possible_truncation)]

use crate::vector_expand::expand_vector_ops;
use crate::{
    MirBinOp, MirBlock, MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic,
    MirLaneOp, MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirProgram,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext, MirValueKind, MirVectorOp, ValidatedMir,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, TypeTable};

const WIDTH: u64 = 3;

struct Fixture {
    types: TypeTable,
    /// Operand vector of the element under test.
    vector: MirType,
    /// A second operand vector of a different (wider) integer element.
    wide_vector: MirType,
    element: MirType,
    /// The vector of `f64` a `÷` over integers produces.
    float_vector: MirType,
}

fn fixture(element: Primitive, width: NumericWidth) -> Fixture {
    let mut types = TypeTable::new();
    let index = types.intern_index(IndexExpr::Literal(WIDTH));
    let element = types.sized_numeric(element, width);
    let wide_element = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let float = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let vector = types.vector_with_width(element, index);
    let wide_vector = types.vector_with_width(wide_element, index);
    let float_vector = types.vector_with_width(float, index);
    Fixture {
        types,
        vector: MirType::semantic(vector),
        wide_vector: MirType::semantic(wide_vector),
        element: MirType::semantic(element),
        float_vector: MirType::semantic(float_vector),
    }
}

fn local(id: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(id),
        ty,
        name: None,
        mutable: true,
        span: Span::default(),
    }
}

fn param(id: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(id),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: Span::default(),
    }
}

fn place(id: u32) -> MirPlace {
    MirPlace::local(MirLocalId(id))
}

/// `out = op(p0, p1)` over parameters of the given types.
fn function(op: MirVectorOp, operands: (MirType, MirType), result: MirType) -> MirFunction {
    let call = MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(place(2)),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::VectorOp(op),
                args: vec![MirOperand::Place(place(0)), MirOperand::Place(place(1))],
                return_ty: result,
            },
        },
        span: Span::default(),
    };
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![param(0, operands.0), param(1, operands.1)],
        locals: vec![local(2, result)],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(place(2)))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: result,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn expanded(fixture: &Fixture, mut function: MirFunction) -> MirFunction {
    let errors = expand_vector_ops(&fixture.types, &mut function);
    assert!(errors.is_empty(), "{errors:?}");
    function
}

fn validates(fixture: &Fixture, function: MirFunction) {
    let program = MirProgram {
        functions: vec![function],
    };
    if let Err(errors) = ValidatedMir::new(program, MirValidationContext::new(&fixture.types)) {
        panic!("{errors:?}");
    }
}

fn statements(function: &MirFunction) -> impl Iterator<Item = &MirStatement> {
    function.blocks.iter().flat_map(|block| &block.statements)
}

fn binaries(function: &MirFunction, wanted: MirBinOp) -> usize {
    statements(function)
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::Assign { value, .. }
                    if matches!(&value.kind, MirValueKind::Binary { op, .. } if *op == wanted)
            )
        })
        .count()
}

fn conversions(function: &MirFunction) -> (usize, usize) {
    let mut plain = 0;
    let mut stores = 0;
    for statement in statements(function) {
        if let MirStatementKind::RuntimeCall { call, .. } = &statement.kind
            && let MirIntrinsic::Convert(conversion) = &call.intrinsic
        {
            if conversion.store.is_some() {
                stores += 1;
            } else {
                plain += 1;
            }
        }
    }
    (plain, stores)
}

fn collection_calls(function: &MirFunction, wanted: MirCollectionOp) -> usize {
    statements(function)
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::RuntimeCall { call, .. }
                    if call.intrinsic == MirIntrinsic::Collection(wanted)
            )
        })
        .count()
}

fn elementwise(op: MirLaneOp, checked: bool) -> MirVectorOp {
    MirVectorOp::Elementwise { op, checked }
}

#[test]
fn integer_true_division_converts_each_lane_and_traps_a_zero_divisor() {
    let fixture = fixture(Primitive::Numerus, NumericWidth::I32);
    let source = function(
        elementwise(MirLaneOp::TrueDiv, false),
        (fixture.vector, fixture.vector),
        fixture.float_vector,
    );
    let function = expanded(&fixture, source);
    let lanes = WIDTH as usize;
    // Per lane: the discarded `1 / divisor` trap and the quotient.
    assert_eq!(binaries(&function, MirBinOp::Div), 2 * lanes);
    // Both operand lanes convert to the result float; no store node: a float
    // result lane has no integer width to check.
    assert_eq!(conversions(&function), (2 * lanes, 0));
    validates(&fixture, function);
}

#[test]
fn float_true_division_is_the_plain_lane_division() {
    let fixture = fixture(Primitive::Fractus, NumericWidth::F32);
    let source = function(
        elementwise(MirLaneOp::TrueDiv, false),
        (fixture.vector, fixture.vector),
        fixture.vector,
    );
    let function = expanded(&fixture, source);
    assert_eq!(binaries(&function, MirBinOp::Div), WIDTH as usize);
    assert_eq!(conversions(&function), (0, 0));
    validates(&fixture, function);
}

#[test]
fn a_scalar_divisor_traps_once_per_lane_and_keeps_the_written_order() {
    let fixture = fixture(Primitive::Numerus, NumericWidth::I32);
    for scalar_first in [false, true] {
        let source = function(
            MirVectorOp::ElementwiseScalar {
                op: MirLaneOp::TrueDiv,
                scalar_first,
                checked: false,
            },
            (fixture.vector, fixture.element),
            fixture.float_vector,
        );
        let function = expanded(&fixture, source);
        assert_eq!(binaries(&function, MirBinOp::Div), 2 * WIDTH as usize);
        validates(&fixture, function);
    }
}

#[test]
fn the_extrema_widen_both_lanes_then_pick_one() {
    // `i32` lanes against `i32` lanes: no widening needed.
    let same = fixture(Primitive::Numerus, NumericWidth::I32);
    for (op, collection) in [
        (MirLaneOp::Max, MirCollectionOp::Maximum),
        (MirLaneOp::Min, MirCollectionOp::Minimum),
    ] {
        let source = function(
            elementwise(op, false),
            (same.vector, same.vector),
            same.vector,
        );
        let function = expanded(&same, source);
        assert_eq!(collection_calls(&function, collection), WIDTH as usize);
        assert_eq!(conversions(&function), (0, 0));
        validates(&same, function);
    }
    // `i8` lanes against `i32` lanes: the narrow lane widens to the result.
    let mixed = fixture(Primitive::Numerus, NumericWidth::I8);
    let source = function(
        elementwise(MirLaneOp::Max, false),
        (mixed.vector, mixed.wide_vector),
        mixed.wide_vector,
    );
    let function = expanded(&mixed, source);
    assert_eq!(
        collection_calls(&function, MirCollectionOp::Maximum),
        WIDTH as usize
    );
    assert_eq!(conversions(&function), (WIDTH as usize, 0));
    validates(&mixed, function);
}

#[test]
fn a_checked_pair_stores_every_lane_under_the_element_type() {
    let fixture = fixture(Primitive::Numerus, NumericWidth::U8);
    for op in [
        MirLaneOp::Add,
        MirLaneOp::Sub,
        MirLaneOp::Mul,
        MirLaneOp::Div,
    ] {
        let source = function(
            elementwise(op, true),
            (fixture.vector, fixture.vector),
            fixture.vector,
        );
        let function = expanded(&fixture, source);
        assert_eq!(conversions(&function), (0, WIDTH as usize), "{op:?}");
        validates(&fixture, function);
    }
}

#[test]
fn an_unchecked_pair_carries_no_store_node() {
    let fixture = fixture(Primitive::Numerus, NumericWidth::U8);
    let source = function(
        elementwise(MirLaneOp::Add, false),
        (fixture.vector, fixture.vector),
        fixture.vector,
    );
    let function = expanded(&fixture, source);
    assert_eq!(conversions(&function), (0, 0));
}
