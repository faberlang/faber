//! inf track U4: the `Magnus` MIR carrier — the appended constant and scalar
//! layout (append-only serde indices), the layout facts, constant typing and
//! validation, and the dump (inf-delivery rulings 34, 44, 46).

use crate::{
    MirBlock, MirBlockId, MirConstant, MirFunction, MirFunctionId, MirLayoutKind, MirLayoutTable,
    MirOperand, MirProgram, MirScalarLayout, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext, validate_program,
};
use radix_lexer::Span;
use radix_types::{NumericWidth, Primitive, TypeTable};

const TWO_64: &str = "18446744073709551616";

fn returning(ty: MirType, constant: MirConstant) -> MirProgram {
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Constant(constant))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    program
}

fn validate(program: &MirProgram, types: &TypeTable) -> Result<(), Vec<String>> {
    let context = MirValidationContext::new(types);
    validate_program(program, &context)
        .map_err(|errors| errors.into_iter().map(|error| error.message).collect())
}

#[test]
fn a_magnus_constant_round_trips_through_json() {
    // (The positional postcard indices are pinned in `radix-mir-fmir`.)
    let constant = MirConstant::Magnus(format!("-{TWO_64}"));
    let json = serde_json::to_string(&constant).expect("json");
    assert_eq!(
        serde_json::from_str::<MirConstant>(&json).expect("decode"),
        constant
    );
    assert!(json.contains("Magnus"), "{json}");
}

#[test]
fn inf_resolves_to_the_unsized_magnus_scalar_layout() {
    let mut types = TypeTable::new();
    let inf = types.sized_numeric(Primitive::Numerus, NumericWidth::Inf);
    let u8_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let fractus_inf = types.sized_numeric(Primitive::Fractus, NumericWidth::Inf);
    let table = MirLayoutTable::from_types(&types);
    let layout = table
        .layout_for_type(MirType::semantic(inf))
        .expect("inf has a layout");
    assert_eq!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::Magnus));
    assert_eq!(layout.size_bits, None, "a heap bignum has no fixed size");
    assert_eq!(layout.align_bits, None);
    let scalar = MirScalarLayout::Magnus;
    assert_eq!(scalar.spelling(), "magnus");
    assert!(!scalar.is_integer() && !scalar.is_float());
    assert!(!scalar.is_signed_integer() && !scalar.is_unsigned_integer());
    assert_eq!(scalar.integer_bits(), None);
    assert!(!MirScalarLayout::U64.represents_integer(scalar));
    assert!(!scalar.represents_integer(MirScalarLayout::U8));
    // A bounded width keeps its fixed scalar, and `fractus<inf>` is not a type.
    let layout = table
        .layout_for_type(MirType::semantic(u8_ty))
        .expect("u8 layout");
    assert_eq!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::U8));
    assert!(
        table
            .layout_for_type(MirType::semantic(fractus_inf))
            .is_err()
    );
}

#[test]
fn a_magnus_constant_types_as_inf_and_validates_only_into_inf() {
    let mut types = TypeTable::new();
    let inf = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::Inf));
    let u64_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U64));
    let int = MirType::semantic(types.primitive(Primitive::Numerus));
    let constant = MirConstant::Magnus(TWO_64.to_owned());

    validate(&returning(inf, constant.clone()), &types).expect("a bignum constant fits inf");
    validate(&returning(inf, MirConstant::Int(-5)), &types).expect("a small constant fits inf");
    validate(&returning(inf, MirConstant::UInt(u64::MAX)), &types).expect("u64::MAX fits inf");

    for bounded in [u64_ty, int] {
        let errors = validate(&returning(bounded, constant.clone()), &types)
            .expect_err("a bignum constant fits no bounded integer");
        assert!(!errors.is_empty());
    }
}

#[test]
fn constant_ty_reads_the_interned_inf_type() {
    let mut types = TypeTable::new();
    let inf = types.sized_numeric(Primitive::Numerus, NumericWidth::Inf);
    let program = MirProgram::new();
    let index = program.function_index();
    let ty = crate::ty::constant_ty_with_index(
        &MirConstant::Magnus(TWO_64.to_owned()),
        &types,
        &program,
        &index,
    )
    .expect("constant type");
    assert_eq!(ty.semantic_id(), inf);
    // Without an interned `inf` the hand-built fallback is a plain integer.
    let bare = TypeTable::new();
    let ty = crate::ty::constant_ty_with_index(
        &MirConstant::Magnus(TWO_64.to_owned()),
        &bare,
        &program,
        &index,
    )
    .expect("constant type");
    assert_eq!(ty.semantic_id(), bare.primitive(Primitive::Numerus));
}

#[test]
fn the_dump_spells_the_bignum_constant() {
    let mut types = TypeTable::new();
    let inf = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::Inf));
    let program = returning(inf, MirConstant::Magnus(TWO_64.to_owned()));
    let dump = crate::dump::dump_program(&program);
    assert!(dump.contains(&format!("const magnus {TWO_64}")), "{dump}");
}
