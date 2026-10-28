//! Step budget: `evaluate_function` reports `StepLimit` only for an attempted
//! visit beyond the budget, never for completion on the last allowed block.

use super::*;

use radix_mir::{MirTerminator, MirType};

#[test]
fn evaluate_function_accepts_completion_on_the_last_budgeted_block() {
    use radix_lexer::Interner;
    use radix_mir::{MirInterner, MirProgram};
    use radix_types::{Primitive, TypeTable};

    let types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let interner = Interner::new();
    let mut context = MirValidationContext::new(&types);
    context.interner = MirInterner::Borrowed(&interner);
    let validated = ValidatedMir::new(
        MirProgram {
            functions: vec![MirFunction {
                id: MirFunctionId(0),
                source: None,
                name: None,
                params: vec![],
                locals: vec![],
                temps: vec![],
                blocks: vec![
                    MirBlock {
                        id: MirBlockId(0),
                        statements: vec![],
                        terminator: MirTerminator {
                            kind: MirTerminatorKind::Goto(MirBlockId(1)),
                            span: Span::default(),
                        },
                        span: Span::default(),
                    },
                    MirBlock {
                        id: MirBlockId(1),
                        statements: vec![],
                        terminator: MirTerminator {
                            kind: MirTerminatorKind::Return(Some(MirOperand::Constant(
                                MirConstant::Int(7),
                            ))),
                            span: Span::default(),
                        },
                        span: Span::default(),
                    },
                ],
                return_ty: numerus,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: Span::default(),
            }],
        },
        context,
    )
    .expect("two-block function fixture validates");

    let mut host = BufferHost::default();
    assert!(matches!(
        evaluate_function(&validated, MirFunctionId(0), &mut host, 1),
        Err(EvaluationFailure::StepLimit)
    ));
    assert_eq!(
        evaluate_function(&validated, MirFunctionId(0), &mut host, 2)
            .expect("completion on the final allowed block is not exhaustion"),
        Value::Int(7)
    );
}
