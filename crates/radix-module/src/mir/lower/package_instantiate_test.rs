use super::*;
use crate::mir::{MirBlock, MirStatement, MirTerminator};

#[test]
fn substitute_function_rewrites_generic_call_type_args() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let symbol = interner.intern("T");
    let generic = MirType::semantic(types.intern(Type::Param(symbol)));
    let concrete = MirType::semantic(types.primitive(Primitive::Textus));
    let mut function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: None,
                    callee: MirCallee::Definition { source: DefId(1), type_args: vec![generic] },
                    args: Vec::new(),
                },
                span: Span::default(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: concrete,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let mut state = PackageInstantiate {
        types: &mut types,
        interner: &interner,
        templates: FxHashMap::default(),
        generic_params: FxHashMap::default(),
        instance_index: FxHashMap::default(),
        monomorphizations: Vec::new(),
        next_id: MirFunctionId(1),
        errors: Vec::new(),
        created: Vec::new(),
    };
    let mut bindings = MirGenericBindings::default();
    bindings.insert(symbol, concrete);

    assert!(state.substitute_function(&mut function, &bindings, Span::default()));
    let MirStatementKind::Call { callee, .. } = &function.blocks[0].statements[0].kind else {
        panic!("expected call statement");
    };
    let MirCallee::Definition { type_args, .. } = callee else {
        panic!("expected definition callee");
    };
    assert_eq!(type_args, &[concrete]);
}
