//! `MirFunctionIndex` resolution, the single function-id allocator, and the
//! node-model helper predicates that sit beside them in `nodes.rs`.

use crate::{
    MirAggregateKind, MirClosureEnvironmentId, MirFunction, MirFunctionId, MirFunctionIdOverflow,
    MirFunctionIndex, MirLiftOp, MirProgram, MirType,
};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_types::TypeId;

fn stub_function(id: u32) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: Vec::new(),
        return_ty: MirType::semantic(TypeId(id)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

#[test]
fn index_resolves_dense_ids() {
    let functions = vec![stub_function(0), stub_function(1), stub_function(2)];
    let index = MirFunctionIndex::for_functions(&functions);
    let found = index
        .function(&functions, MirFunctionId(1))
        .expect("dense hit");
    assert_eq!(found.id, MirFunctionId(1));
    assert_eq!(found.return_ty.semantic_id(), TypeId(1));
}

#[test]
fn index_resolves_sparse_ids() {
    // After template retain, vector index need not equal id.0.
    let functions = vec![stub_function(0), stub_function(5), stub_function(9)];
    let index = MirFunctionIndex::for_functions(&functions);
    let found = index
        .function(&functions, MirFunctionId(9))
        .expect("sparse hit");
    assert_eq!(found.id, MirFunctionId(9));
    assert!(index.function(&functions, MirFunctionId(1)).is_none());
}

#[test]
fn index_mut_rewrites_dense_and_sparse() {
    let mut functions = vec![stub_function(0), stub_function(3)];
    let index = MirFunctionIndex::for_functions(&functions);
    {
        let f = index
            .function_mut(&mut functions, MirFunctionId(0))
            .expect("dense mut");
        f.name = Some(Symbol(1));
    }
    {
        let f = index
            .function_mut(&mut functions, MirFunctionId(3))
            .expect("sparse mut");
        f.name = Some(Symbol(2));
    }
    assert_eq!(functions[0].name, Some(Symbol(1)));
    assert_eq!(functions[1].name, Some(Symbol(2)));
    assert!(
        index
            .function_mut(&mut functions, MirFunctionId(2))
            .is_none()
    );
}

#[test]
fn index_records_appended_functions() {
    let mut functions = vec![stub_function(0)];
    let mut index = MirFunctionIndex::for_functions(&functions);
    functions.push(stub_function(7));
    index.insert(MirFunctionId(7), 1);
    let found = index
        .function(&functions, MirFunctionId(7))
        .expect("appended hit");
    assert_eq!(found.id, MirFunctionId(7));
}

#[test]
fn allocate_function_id_derives_max_plus_one() {
    assert_eq!(
        MirProgram::allocate_function_id(&[]).expect("empty set"),
        MirFunctionId(0)
    );
    let functions = vec![stub_function(0), stub_function(3), stub_function(9)];
    assert_eq!(
        MirProgram::allocate_function_id(&functions).expect("non-empty set"),
        MirFunctionId(10)
    );
}

#[test]
fn allocate_function_id_fails_closed_on_overflow() {
    let functions = vec![stub_function(u32::MAX)];
    assert_eq!(
        MirProgram::allocate_function_id(&functions),
        Err(MirFunctionIdOverflow)
    );
}

/// Every lift operator reports the operand count of its scalar form, so a new
/// variant cannot quietly inherit a wrong arity through a wildcard arm.
#[test]
fn lift_arity_matches_the_operand_count_each_op_takes() {
    assert_eq!(MirLiftOp::Neg.arity(), 1);
    assert_eq!(MirLiftOp::Not.arity(), 1);
    assert_eq!(MirLiftOp::Select.arity(), 3);
    for op in [
        MirLiftOp::Add,
        MirLiftOp::Sub,
        MirLiftOp::Mul,
        MirLiftOp::Div,
        MirLiftOp::Mod,
        MirLiftOp::TrueDiv,
        MirLiftOp::Eq,
        MirLiftOp::NotEq,
        MirLiftOp::Lt,
        MirLiftOp::Gt,
        MirLiftOp::LtEq,
        MirLiftOp::GtEq,
        MirLiftOp::And,
        MirLiftOp::Or,
        MirLiftOp::Coalesce,
        MirLiftOp::Max,
        MirLiftOp::Min,
        MirLiftOp::WholeEq,
        MirLiftOp::WholeNotEq,
    ] {
        assert_eq!(op.arity(), 2, "{op:?}");
    }
}

/// Only the def-backed aggregate kinds carry a `DefId`; every other kind must
/// report `None` by its own name rather than through a wildcard arm.
#[test]
fn aggregate_definition_id_is_none_for_every_kind_without_a_def() {
    for kind in [
        MirAggregateKind::Tuple,
        MirAggregateKind::Array,
        MirAggregateKind::Record,
        MirAggregateKind::ClosureEnvironment(MirClosureEnvironmentId(0)),
        MirAggregateKind::SharedCell,
        MirAggregateKind::Map,
        MirAggregateKind::Set,
        MirAggregateKind::Tensor,
        MirAggregateKind::SparseTensor,
        MirAggregateKind::Interval { inclusive: true },
        MirAggregateKind::Vector,
        MirAggregateKind::Matrix,
    ] {
        assert_eq!(kind.definition_id(), None, "{kind:?}");
    }
    assert_eq!(
        MirAggregateKind::Struct(DefId(7)).definition_id(),
        Some(DefId(7))
    );
    assert_eq!(
        MirAggregateKind::EnumVariant(DefId(9)).definition_id(),
        Some(DefId(9))
    );
}
