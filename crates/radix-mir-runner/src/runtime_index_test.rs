use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext};
use radix_types::{TypeId, TypeTable};
use rustc_hash::FxHashMap;

#[test]
fn missing_tabula_key_traps_only_for_strict_index() {
    let type_table = TypeTable::new();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    let receiver = Value::Map(Rc::new(RefCell::new(FxHashMap::default())));
    let missing_key = Value::Str("missing".into());

    assert!(
        runner
            .collection_index(receiver.clone(), missing_key.clone(), Span::default())
            .is_err()
    );
    assert_eq!(
        runner
            .optional_collection_index(receiver, missing_key, Span::default())
            .expect("optional map miss"),
        Value::Nil
    );
}
