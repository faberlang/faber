//! `ValidatedMir::detach` / `DetachedMir::attach`: a validated program moves to
//! another thread as an owned, `Send` value and re-attaches there.

use super::*;
use crate::{
    MirBlock, MirBlockId, MirFunction, MirFunctionId, MirLocal, MirLocalId, MirProgram,
    MirTerminator, MirTerminatorKind, MirType,
};
use radix_lexer::Span;
use radix_types::{Primitive, TypeId, TypeTable};

fn assert_send<T: Send>() {}

fn trivial_program(ty: TypeId) -> MirProgram {
    let span = Span::default();
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: MirType::semantic(ty),
                mutable: false,
                span,
            }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span,
                },
                span,
            }],
            return_ty: MirType::semantic(ty),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }],
    }
}

#[test]
fn detached_mir_is_send() {
    assert_send::<DetachedMir>();
}

#[test]
fn detached_mir_attaches_on_another_thread() {
    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let validated = ValidatedMir::new(trivial_program(vacuum), MirValidationContext::new(&types))
        .expect("program validates");

    let expected_ty = types.get(vacuum).clone();
    let detached = validated.detach().expect("borrowed table detaches");
    let (found, resolved) = std::thread::spawn(move || {
        let attached = detached.attach();
        let found = attached.function_by_id(MirFunctionId(0)).is_some();
        let resolved = *attached.validation().types.get(vacuum) == expected_ty;
        (found, resolved)
    })
    .join()
    .expect("worker thread");

    assert!(found, "function resolves through the rebuilt index");
    assert!(resolved, "type resolves through the owned table");
    assert!(
        validated.function_by_id(MirFunctionId(0)).is_some(),
        "the source token is unaffected"
    );
}

#[test]
fn detach_accepts_owned_interner_and_refuses_borrowed() {
    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let interner = radix_lexer::Interner::new();

    let mut context = MirValidationContext::new(&types);
    context.interner = MirInterner::Borrowed(&interner);
    let borrowed = ValidatedMir::new(trivial_program(vacuum), context).expect("validates");
    let Err(error) = borrowed.detach() else {
        panic!("borrowed interner must be refused");
    };
    assert!(error.contains("interner"), "{error}");

    let mut context = MirValidationContext::new(&types);
    context.interner = MirInterner::Owned(std::sync::Arc::new(interner));
    let owned = ValidatedMir::new(trivial_program(vacuum), context).expect("validates");
    assert!(owned.detach().is_ok());
}
