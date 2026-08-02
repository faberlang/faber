//! Reject regression tests for `ValidatedMir` (PRP-010 Stage 0).
//!
//! Each test constructs a `MirProgram` bearing a known validation error and
//! proves that `ValidatedMir::new` rejects it. `ValidatedMir` is the single
//! validation gate consumed by every public MIR entry point.

use radix_lexer::Span;
use radix_mir::{
    MirBlock, MirBlockId, MirFunction, MirFunctionId, MirProgram, MirTerminator, MirTerminatorKind,
    MirType, MirValidationContext, ValidatedMir,
};
use radix_types::{Primitive, TypeId, TypeTable};

// ── Builder helpers ─────────────────────────────────────────────────────────

fn block(id: u32, terminator: MirTerminatorKind) -> MirBlock {
    MirBlock {
        id: MirBlockId(id),
        statements: vec![],
        terminator: MirTerminator {
            kind: terminator,
            span: Span::default(),
        },
        span: Span::default(),
    }
}

fn func(id: u32, blocks: Vec<MirBlock>, return_ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks,
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn assert_rejected(program: MirProgram, types: &TypeTable) -> Vec<String> {
    let context = MirValidationContext::new(types);
    match ValidatedMir::new(program, context) {
        Ok(_) => panic!("expected ValidatedMir::new to reject invalid program"),
        Err(errors) => errors.into_iter().map(|e| e.message).collect(),
    }
}

// ── Reject regression tests ────────────────────────────────────────────────

#[test]
fn rejects_no_value_return() {
    // Function has a non-unit return type but Return carries no value.
    let types = TypeTable::default();
    let prog = MirProgram {
        functions: vec![func(
            0,
            vec![block(0, MirTerminatorKind::Return(None))],
            MirType::semantic(TypeId(0)),
        )],
    };
    let messages = assert_rejected(prog, &types);
    assert!(
        messages.iter().any(|m| m.contains("no-value return")),
        "expected no-value-return rejection, got: {messages:?}"
    );
}

#[test]
fn rejects_missing_block_target() {
    // Goto target block ID 999 does not exist.
    let types = TypeTable::default();
    let prog = MirProgram {
        functions: vec![func(
            0,
            vec![block(0, MirTerminatorKind::Goto(MirBlockId(999)))],
            MirType::semantic(TypeId(0)),
        )],
    };
    let messages = assert_rejected(prog, &types);
    assert!(
        messages.iter().any(|m| m.contains("block")),
        "expected block-target rejection, got: {messages:?}"
    );
}

/// Empty-context substitution is impossible: `ValidatedMir::new` still
/// validates the program against the context it consumes. An invalid program
/// is rejected even when the context carries no extra metadata.
#[test]
fn rejects_empty_context_with_duplicate_function_id() {
    let types = TypeTable::default();
    let nihil = types.primitive(Primitive::Nihil);
    let empty = MirValidationContext::new(&types);

    let blk = block(0, MirTerminatorKind::Return(None));
    let f = func(0, vec![blk], MirType::semantic(nihil));
    let program = MirProgram {
        functions: vec![f.clone(), f],
    };

    let result = ValidatedMir::new(program, empty);
    assert!(
        result.is_err(),
        "empty context must still validate and reject duplicate function IDs"
    );
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|e| e.message.contains("duplicate")),
        "expected duplicate function ID error, got: {errors:?}"
    );
}

// ── Edge cases ─────────────────────────────────────────────────────────────

#[test]
fn accepts_empty_program() {
    // Zero functions represents a valid no-entry artifact.
    let types = TypeTable::default();
    let prog = MirProgram { functions: vec![] };
    let context = MirValidationContext::new(&types);
    ValidatedMir::new(prog, context).expect("empty MIR program should validate");
}

#[test]
fn rejects_function_with_no_blocks() {
    // Every function must have at least one block.
    let types = TypeTable::default();
    let prog = MirProgram {
        functions: vec![func(
            0,
            vec![],
            MirType::semantic(types.primitive(Primitive::Nihil)),
        )],
    };
    let messages = assert_rejected(prog, &types);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("block") || m.contains("empty")),
        "expected block/empty rejection for function with no blocks, got: {messages:?}"
    );
}

#[test]
fn rejects_duplicate_block_ids_within_function() {
    // Two blocks share the same ID.
    let types = TypeTable::default();
    let prog = MirProgram {
        functions: vec![func(
            0,
            vec![
                block(0, MirTerminatorKind::Goto(MirBlockId(1))),
                block(0, MirTerminatorKind::Return(None)),
            ],
            MirType::semantic(types.primitive(Primitive::Nihil)),
        )],
    };
    let messages = assert_rejected(prog, &types);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("duplicate") || m.contains("block")),
        "expected duplicate block ID rejection, got: {messages:?}"
    );
}
