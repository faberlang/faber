//! Scalar negation of a mutable parameter reads its numeric payload place.
//!
//! HIR retains the `mut` access marker on the parameter expression, but MIR
//! stores the parameter local with its numeric payload type. Negation must
//! materialize that place as a payload-typed read and produce a payload-typed
//! result for float, integer, and decimal parameters.

use super::*;
use radix_hir::HirId;
use radix_types::SemanticMutability;

const PARAM: DefId = DefId(1);

#[test]
fn negation_reads_mut_scalar_params_with_their_numeric_type() {
    let mut types = TypeTable::new();
    let float = types.primitive(Primitive::Fractus);
    let integer = types.primitive(Primitive::Numerus);
    let decimal = types.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::D64));
    let cases = [float, integer, decimal].map(|ty| (ty, types.reference(SemanticMutability::Mutable, ty)));
    let mut interner = Interner::new();
    let name = interner.intern("p");

    for (numeric_ty, marked_ty) in cases {
        let mut builder = FunctionBuilder::new(&types);
        builder.add_param(PARAM, name, marked_ty, HirParamMode::In, Span::default());
        let entry = builder.fresh_block(Span::default());
        builder.switch_to(entry);
        let parameter = builder.params[0].local;
        let operand = HirExpression {
            id: HirId(1),
            kind: HirExpressionKind::Path(PARAM),
            ty: Some(marked_ty),
            span: Span::default(),
        };
        let expression = HirExpression {
            id: HirId(2),
            kind: HirExpressionKind::Unary(HirUnOp::Neg, Box::new(operand.clone())),
            ty: Some(marked_ty),
            span: Span::default(),
        };

        let result = builder
            .lower_unary(HirUnOp::Neg, &operand, &expression)
            .expect("negation lowers");
        assert!(builder.errors.is_empty(), "unexpected lowering errors: {:?}", builder.errors);
        let MirOperand::Temp(result_temp) = result else {
            panic!("negation result is a typed temp, got {result:?}");
        };
        let block_index = usize::try_from(entry.0).expect("block id fits usize");
        let statements = &builder.blocks[block_index].statements;
        assert_eq!(statements.len(), 2, "marked input is read before negation");

        let MirStatementKind::Assign { place: read_place, value: read } = &statements[0].kind else {
            panic!("first statement reads the parameter place");
        };
        let MirPlaceBase::Temp(read_temp) = &read_place.base else {
            panic!("the typed parameter read is assigned to a temp");
        };
        assert_eq!(read.ty, MirType::semantic(numeric_ty));
        assert!(matches!(
            &read.kind,
            MirValueKind::Operand(MirOperand::Place(place)) if *place == MirPlace::local(parameter)
        ));

        let MirStatementKind::Assign { place, value: negated } = &statements[1].kind else {
            panic!("second statement negates the typed read");
        };
        assert_eq!(*place, MirPlace::temp(result_temp));
        assert_eq!(negated.ty, MirType::semantic(numeric_ty));
        assert!(matches!(
            &negated.kind,
            MirValueKind::Unary { op: MirUnOp::Neg, operand: MirOperand::Temp(temp) } if temp == read_temp
        ));
    }
}
