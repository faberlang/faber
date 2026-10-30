//! The explicit numeric conversion matrix: which source/target pairs lower,
//! and which stay declined by name.

use super::rep_of;
use super::{MemberTest, Num, Seq, member_tests, numeric_conversion, sequence_conversion};
use crate::{StructMeta, emit_program};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlock, MirBlockId, MirConstant, MirConversion, MirConversionFlavor, MirFunction,
    MirFunctionId, MirIntrinsic, MirLocal, MirLocalId, MirOperand, MirPlace, MirProgram,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
};
use radix_types::{BitsWidth, NumericWidth, Primitive, Type, TypeId, TypeTable};

struct Fixture {
    types: TypeTable,
}

impl Fixture {
    fn new() -> Self {
        Self {
            types: TypeTable::new(),
        }
    }

    fn primitive(&self, primitive: Primitive) -> TypeId {
        self.types.primitive(primitive)
    }

    fn sized(&mut self, primitive: Primitive, width: NumericWidth) -> TypeId {
        self.types.intern(Type::SizedNumeric(primitive, width))
    }

    fn modular(&mut self, width: NumericWidth) -> TypeId {
        self.types.intern(Type::ModularWord(width))
    }

    fn saturating(&mut self, width: NumericWidth) -> TypeId {
        self.types.intern(Type::SaturatingWord(width))
    }

    fn bits(&mut self, width: BitsWidth) -> TypeId {
        self.types.intern(Type::Bits(width))
    }

    fn supported(&self, source: TypeId, target: TypeId) -> bool {
        numeric_conversion(&self.types, source, target).is_some()
    }
}

#[test]
fn integer_narrowing_widening_and_policy_words_lower() {
    let mut f = Fixture::new();
    let int = f.primitive(Primitive::Numerus);
    let u8_ = f.sized(Primitive::Numerus, NumericWidth::U8);
    let i8_ = f.sized(Primitive::Numerus, NumericWidth::I8);
    let wrapping_i8 = f.modular(NumericWidth::I8);
    let wrapping_u64 = f.modular(NumericWidth::U64);
    let saturating_u8 = f.saturating(NumericWidth::U8);
    assert!(f.supported(int, u8_));
    assert!(f.supported(u8_, int));
    assert!(f.supported(i8_, u8_));
    assert!(f.supported(int, wrapping_i8));
    assert!(f.supported(int, saturating_u8));
    // A wrapping u64 result leaves the i64 carrier.
    assert!(!f.supported(int, wrapping_u64));
}

#[test]
fn float_text_and_bool_pairs() {
    let mut f = Fixture::new();
    let int = f.primitive(Primitive::Numerus);
    let float = f.primitive(Primitive::Fractus);
    let text = f.primitive(Primitive::Textus);
    let boolean = f.primitive(Primitive::Bivalens);
    let u64_ = f.sized(Primitive::Numerus, NumericWidth::U64);
    let wrapping_u8 = f.modular(NumericWidth::U8);
    assert!(f.supported(float, int));
    assert!(f.supported(text, int));
    assert!(f.supported(text, float));
    assert!(f.supported(text, boolean));
    assert!(f.supported(int, boolean));
    assert!(f.supported(boolean, float));
    // The u64 range exceeds the signed carrier: declined, not approximated.
    assert!(!f.supported(float, u64_));
    assert!(!f.supported(text, u64_));
    // Float and text sources never feed a policy word here.
    assert!(!f.supported(float, wrapping_u8));
    assert!(!f.supported(int, text));
}

#[test]
fn raw_bits_pair_only_with_same_width_scalars() {
    let mut f = Fixture::new();
    let i32_ = f.sized(Primitive::Numerus, NumericWidth::I32);
    let u32_ = f.sized(Primitive::Numerus, NumericWidth::U32);
    let u64_ = f.sized(Primitive::Numerus, NumericWidth::U64);
    let f32_ = f.sized(Primitive::Fractus, NumericWidth::F32);
    let f16_ = f.sized(Primitive::Fractus, NumericWidth::F16);
    let b32 = f.bits(BitsWidth::B32);
    let b16 = f.bits(BitsWidth::B16);
    let b64 = f.bits(BitsWidth::B64);
    assert!(f.supported(i32_, b32));
    assert!(f.supported(b32, u32_));
    assert!(f.supported(f32_, b32));
    assert!(f.supported(b32, f32_));
    assert!(!f.supported(i32_, b16));
    assert!(f.supported(f16_, b16));
    assert!(f.supported(b16, f16_));
    assert!(f.supported(u64_, b64));
    assert!(!f.supported(b64, u64_));
    assert!(matches!(
        numeric_conversion(&f.types, i32_, b32).map(|plan| plan.target()),
        Some(Num::Bits(BitsWidth::B32))
    ));
}

#[test]
fn union_targets_test_each_member_and_reject_unlowered_members() {
    let mut f = Fixture::new();
    let int = f.primitive(Primitive::Numerus);
    let text = f.primitive(Primitive::Textus);
    let none = f.primitive(Primitive::Nihil);
    let regex = f.primitive(Primitive::Regex);
    let mixed = f.types.intern(Type::Union(vec![int, text]));
    let with_none = f.types.intern(Type::Union(vec![int, none]));
    let json = f.primitive(Primitive::Json);
    let with_json = f.types.intern(Type::Union(vec![int, json]));
    // Union members are stored in id order, so the tests come in either order.
    let tests = member_tests(&f.types, mixed).expect("a union of scalars tests by tag");
    assert_eq!(tests.len(), 2);
    assert!(tests.contains(&MemberTest::Tag(crate::abi::TAG_INT)));
    assert!(tests.contains(&MemberTest::Tag(crate::abi::TAG_TEXT)));
    assert_eq!(member_tests(&f.types, none), Some(vec![MemberTest::Nil]));
    assert_eq!(member_tests(&f.types, regex), None);
    // An unbounded integer is an integer in `i64` (plain tag) or beyond it.
    let inf = f.sized(Primitive::Numerus, NumericWidth::Inf);
    assert_eq!(
        member_tests(&f.types, inf),
        Some(vec![
            MemberTest::Tag(crate::abi::TAG_INT),
            MemberTest::Tag(crate::abi::TAG_INF)
        ])
    );
    let d64 = f.sized(Primitive::Numerus, NumericWidth::D64);
    assert_eq!(member_tests(&f.types, d64), None);
    // A boxed union lowers; a none member would collide with the optional's
    // null and a member outside the subset is refused.
    assert!(rep_of(&f.types, MirType::semantic(mixed)).is_ok());
    assert!(rep_of(&f.types, MirType::semantic(with_none)).is_err());
    assert!(rep_of(&f.types, MirType::semantic(with_json)).is_err());
}

#[test]
fn bounded_text_bytes_and_regrouped_lists_plan_by_type() {
    let mut f = Fixture::new();
    let capacity = f.types.intern_index(radix_types::IndexExpr::Literal(8));
    let text = f.primitive(Primitive::Textus);
    let int = f.primitive(Primitive::Numerus);
    let bounded_text = f.types.bounded_textus(capacity);
    let bounded_ascii = f.types.bounded_ascii(capacity);
    let plan = |f: &Fixture, from, to| sequence_conversion(&f.types, from, to);
    assert_eq!(
        plan(&f, text, bounded_text),
        Some(Seq::BoundText { capacity: 8 })
    );
    assert_eq!(plan(&f, bounded_text, text), Some(Seq::Release));
    assert_eq!(plan(&f, bounded_ascii, text), Some(Seq::Release));
    assert_eq!(
        plan(&f, int, bounded_ascii),
        Some(Seq::Pack { capacity: 8 })
    );
    // Text never narrows to ascii by this plan.
    assert_eq!(plan(&f, text, bounded_ascii), None);
    let b8 = f.bits(BitsWidth::B8);
    let u16_ = f.sized(Primitive::Numerus, NumericWidth::U16);
    let u64_ = f.sized(Primitive::Numerus, NumericWidth::U64);
    let bytes = f.types.array(b8);
    let words = f.types.array(u16_);
    let wide = f.types.array(u64_);
    assert_eq!(
        plan(&f, bytes, words),
        Some(Seq::Rewidth {
            from: 8,
            to: 16,
            kind: 1
        })
    );
    assert_eq!(
        plan(&f, words, bytes),
        Some(Seq::Rewidth {
            from: 16,
            to: 8,
            kind: 0
        })
    );
    // A 64-bit unsigned element does not fit the carrier.
    assert_eq!(plan(&f, bytes, wide), None);
}

#[test]
fn byte_lists_read_as_endian_scalars_and_not_as_u64() {
    let mut f = Fixture::new();
    let b8 = f.bits(BitsWidth::B8);
    let bytes = f.types.array(b8);
    let u32_ = f.sized(Primitive::Numerus, NumericWidth::U32);
    let i16_ = f.sized(Primitive::Numerus, NumericWidth::I16);
    let u64_ = f.sized(Primitive::Numerus, NumericWidth::U64);
    let f32_ = f.sized(Primitive::Fractus, NumericWidth::F32);
    let plan = |f: &Fixture, to| sequence_conversion(&f.types, bytes, to);
    assert_eq!(
        plan(&f, u32_),
        Some(Seq::ReadScalar {
            bytes: 4,
            signed: false,
            float: false
        })
    );
    assert_eq!(
        plan(&f, i16_),
        Some(Seq::ReadScalar {
            bytes: 2,
            signed: true,
            float: false
        })
    );
    assert_eq!(
        plan(&f, f32_),
        Some(Seq::ReadScalar {
            bytes: 4,
            signed: false,
            float: true
        })
    );
    // A 64-bit unsigned value leaves the i64 carrier.
    assert_eq!(plan(&f, u64_), None);
}

#[test]
fn integer_intervals_are_values_and_others_are_not() {
    let mut f = Fixture::new();
    let int = f.primitive(Primitive::Numerus);
    let float = f.primitive(Primitive::Fractus);
    let ints = f.types.intervallum(int);
    let floats = f.types.intervallum(float);
    assert!(rep_of(&f.types, MirType::semantic(ints)).is_ok());
    assert!(rep_of(&f.types, MirType::semantic(floats)).is_err());
}

#[test]
fn scalars_write_into_a_byte_buffer_of_their_width() {
    let mut f = Fixture::new();
    f.bits(BitsWidth::B8);
    let u16_ = f.sized(Primitive::Numerus, NumericWidth::U16);
    let f32_ = f.sized(Primitive::Fractus, NumericWidth::F32);
    let two = f.types.intern_index(radix_types::IndexExpr::Literal(2));
    let four = f.types.intern_index(radix_types::IndexExpr::Literal(4));
    let bytes2 = f.types.bounded_octeti(two);
    let bytes4 = f.types.bounded_octeti(four);
    let plan = |f: &Fixture, from, to| sequence_conversion(&f.types, from, to);
    assert_eq!(
        plan(&f, u16_, bytes2),
        Some(Seq::WriteScalar {
            bytes: 2,
            float: false
        })
    );
    assert_eq!(
        plan(&f, f32_, bytes4),
        Some(Seq::WriteScalar {
            bytes: 4,
            float: true
        })
    );
    // The capacity must equal the scalar's width.
    assert_eq!(plan(&f, u16_, bytes4), None);
}

#[test]
fn letters_convert_to_and_from_numbers_and_texts() {
    let mut f = Fixture::new();
    let letter = f.primitive(Primitive::Littera);
    let text = f.primitive(Primitive::Textus);
    let int = f.primitive(Primitive::Numerus);
    let u32_ = f.sized(Primitive::Numerus, NumericWidth::U32);
    let wrapping_u8 = f.modular(NumericWidth::U8);
    assert!(f.supported(text, letter));
    assert!(f.supported(int, letter));
    assert!(f.supported(letter, int));
    assert!(f.supported(letter, u32_));
    // A letter becomes text through the display path, not a plan.
    assert!(!f.supported(letter, text));
    // A policy word target stays declined.
    assert!(!f.supported(letter, wrapping_u8));
    assert_eq!(
        rep_of(&f.types, MirType::semantic(letter)),
        Ok(super::Rep::Char)
    );
}

#[test]
fn decimals_convert_with_integers_floats_and_texts() {
    let mut f = Fixture::new();
    let d64 = f.sized(Primitive::Numerus, NumericWidth::D64);
    let int = f.primitive(Primitive::Numerus);
    let u64_ = f.sized(Primitive::Numerus, NumericWidth::U64);
    let float = f.primitive(Primitive::Fractus);
    let text = f.primitive(Primitive::Textus);
    assert!(f.supported(int, d64));
    assert!(f.supported(float, d64));
    assert!(f.supported(text, d64));
    assert!(f.supported(d64, int));
    assert!(f.supported(d64, float));
    assert!(f.supported(d64, d64));
    // A 64-bit unsigned cell reaches beyond the carrier, and a decimal does
    // not render through the numeric plans (it renders as text directly).
    assert!(!f.supported(u64_, d64));
    assert!(!f.supported(d64, u64_));
    assert!(!f.supported(d64, text));
}

#[test]
fn open_data_and_unbounded_integers_convert_by_tag_and_range() {
    let mut f = Fixture::new();
    let inf = f.sized(Primitive::Numerus, NumericWidth::Inf);
    let int = f.primitive(Primitive::Numerus);
    let u8_ = f.sized(Primitive::Numerus, NumericWidth::U8);
    let u64_ = f.sized(Primitive::Numerus, NumericWidth::U64);
    let valor = f.primitive(Primitive::Valor);
    let text = f.primitive(Primitive::Textus);
    let float = f.primitive(Primitive::Fractus);
    assert!(f.supported(int, inf));
    assert!(f.supported(inf, u8_));
    assert!(f.supported(inf, valor));
    assert!(f.supported(valor, inf));
    assert!(f.supported(int, valor));
    assert!(f.supported(valor, text));
    assert!(f.supported(valor, float));
    // A 64-bit unsigned cell reaches beyond the carrier.
    assert!(!f.supported(inf, u64_));
    assert!(!f.supported(valor, u64_));
}

fn ty(types: &TypeTable, primitive: Primitive) -> MirType {
    MirType::semantic(types.primitive(primitive))
}

/// A bare `<textus> ↦ bool` parses the code locale's words; it is not a
/// non-empty length test.
#[test]
fn text_bivalens_lowers_to_the_word_parse_row() {
    let types = TypeTable::new();
    let text = ty(&types, Primitive::Textus);
    let boolean = ty(&types, Primitive::Bivalens);
    let vacuum = ty(&types, Primitive::Vacuum);
    let mut interner = Interner::new();
    let word = interner.intern("true");
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: boolean,
                mutable: false,
                span: Span::default(),
            }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace::local(MirLocalId(0))),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::Convert(MirConversion {
                                store: None,
                                flavor: MirConversionFlavor::Runtime,
                                source_ty: text,
                                target_ty: boolean,
                                params: Vec::new(),
                                // The `⊥` default keeps the raise path out of the
                                // object; that path names the text length for its trap.
                                recovery: Some(MirOperand::Constant(MirConstant::Bool(false))),
                                struct_defaults: Vec::new(),
                            }),
                            args: vec![MirOperand::Constant(MirConstant::String(word))],
                            return_ty: boolean,
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
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let object = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    )
    .expect("the text-to-bivalens conversion lowers");

    let names = |name: &str| {
        object
            .bytes
            .windows(name.len())
            .any(|window| window == name.as_bytes())
    };
    assert!(
        names("__faber_clif_v1_text_parse_bool"),
        "the object names the word parse"
    );
    assert!(
        !names("__faber_clif_v1_text_len"),
        "the object does not test the text length"
    );
}
