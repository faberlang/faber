//! `MirConstant` Hash/Eq contract: values that compare equal hash equal.

use crate::MirConstant;
use crate::names::MirNames;
use crate::{
    MirBlock, MirBlockId, MirFunction, MirFunctionId, MirProgram, MirTerminator, MirTerminatorKind,
    MirType,
};
use radix_lexer::{Interner, Span};
use radix_types::{Primitive, TypeTable};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn hash_of(constant: &MirConstant) -> u64 {
    let mut hasher = DefaultHasher::new();
    constant.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn signed_zero_float_constants_hash_equal_because_they_compare_equal() {
    let positive = MirConstant::Float(0.0);
    let negative = MirConstant::Float(-0.0);
    assert_eq!(positive, negative, "derived PartialEq treats ±0.0 as equal");
    assert_eq!(
        hash_of(&positive),
        hash_of(&negative),
        "equal constants must hash equal"
    );
}

#[test]
fn distinct_float_constants_keep_distinct_hashes() {
    assert_ne!(
        hash_of(&MirConstant::Float(0.0)),
        hash_of(&MirConstant::Float(1.0))
    );
}

fn entry_candidate(id: u32, return_ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

#[test]
fn explicit_entry_function_finds_numerus_returning_sourceless_entry() {
    let types = TypeTable::new();
    let interner = Interner::new();
    let program = MirProgram {
        functions: vec![entry_candidate(
            0,
            MirType::semantic(types.primitive(Primitive::Numerus)),
        )],
    };
    let names = MirNames::new(&program, &types, &interner);

    let explicit = names
        .explicit_entry_function()
        .expect("numerus-returning sourceless entry must be found");
    assert_eq!(explicit.id, MirFunctionId(0));
    let inherited = names
        .entry_function()
        .expect("entry_function must inherit the numerus fallback");
    assert_eq!(inherited.id, MirFunctionId(0));
}

#[test]
fn explicit_entry_function_prefers_vacuum_candidate_over_numerus() {
    let types = TypeTable::new();
    let interner = Interner::new();
    let program = MirProgram {
        functions: vec![
            entry_candidate(0, MirType::semantic(types.primitive(Primitive::Vacuum))),
            entry_candidate(1, MirType::semantic(types.primitive(Primitive::Numerus))),
        ],
    };
    let names = MirNames::new(&program, &types, &interner);

    let explicit = names
        .explicit_entry_function()
        .expect("vacuum candidate must win");
    assert_eq!(explicit.id, MirFunctionId(0));
}
