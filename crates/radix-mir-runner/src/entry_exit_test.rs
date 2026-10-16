//! Tests for `run_entry_with_process_exit`: a `numerus` entry's returned
//! value becomes the process exit status, while a vacuum entry returns
//! `Ok(())` without reaching `Host::exit`.

use super::{BufferHost, RunnerKernelPolicy, run_entry_with_process_exit};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlock, MirBlockId, MirConstant, MirFunction, MirFunctionId, MirInterner, MirOperand,
    MirProgram, MirTerminator, MirTerminatorKind, MirType, MirValidationContext, ValidatedMir,
};
use radix_types::{Primitive, TypeTable};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn single_block_program(return_ty: MirType, terminator: MirTerminatorKind) -> MirProgram {
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: terminator,
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
        }],
    }
}

#[test]
fn numerus_entry_return_becomes_the_process_exit_status() {
    let types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let program = single_block_program(
        numerus,
        MirTerminatorKind::Return(Some(MirOperand::Constant(MirConstant::Int(7)))),
    );
    let interner = Interner::new();
    let mut context = MirValidationContext::new(&types);
    context.interner = MirInterner::Borrowed(&interner);
    let validated =
        ValidatedMir::new(program, context).expect("numerus exit-entry fixture must validate");
    let mut host = BufferHost::default();

    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = run_entry_with_process_exit(&validated, &mut host, RunnerKernelPolicy::Refuse);
    }))
    .expect_err("a numerus entry must reach Host::exit");
    let message = panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
        .unwrap_or_default();
    assert_eq!(message, "runner exit: 7");
}

#[test]
fn vacuum_entry_returns_without_exiting() {
    let types = TypeTable::new();
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let program = single_block_program(vacuum, MirTerminatorKind::Return(None));
    let interner = Interner::new();
    let mut context = MirValidationContext::new(&types);
    context.interner = MirInterner::Borrowed(&interner);
    let validated =
        ValidatedMir::new(program, context).expect("vacuum exit-entry fixture must validate");
    let mut host = BufferHost::default();

    run_entry_with_process_exit(&validated, &mut host, RunnerKernelPolicy::Refuse)
        .expect("a vacuum entry returns Ok without exiting");
}
