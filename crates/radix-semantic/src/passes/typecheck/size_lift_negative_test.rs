use super::*;
use radix_lexer::{Interner, Span};
use radix_target::Target;

fn magnitudo_params(names: &[radix_lexer::Symbol]) -> Vec<radix_types::GenericParamDescriptor> {
    names
        .iter()
        .map(|name| radix_types::GenericParamDescriptor {
            name: *name,
            sort: radix_types::GenericParamSort::Magnitudo,
            constraint: radix_types::TypeParamConstraint::Any,
            default: None,
        })
        .collect()
}

#[test]
fn negative_constant_size_arg_is_a_compile_error_at_the_call() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut interner = Interner::new();
    let h = interner.intern("h");
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let zero = checker.types.intern_index(IndexExpr::Literal(0));
    let witness = checker.types.intern_index(IndexExpr::Affine {
        base: zero,
        offset: -1,
    });
    checker.current_shape_call = Some(HirId(7));
    let mut shape_vars = FxHashMap::default();
    shape_vars.insert(h, witness);
    let params = magnitudo_params(&[h]);

    let runtime_symbols =
        checker.capture_runtime_call_size_bindings(&params, &shape_vars, Span::default());
    checker.reject_negative_constant_size_args(
        &params,
        &shape_vars,
        &runtime_symbols,
        Span::default(),
    );

    assert!(runtime_symbols.is_empty());
    assert!(
        checker
            .shape_obligations
            .call_size_bindings
            .get(&HirId(7))
            .is_none()
    );
    assert!(
        checker
            .errors
            .iter()
            .any(|error| { error.issue() == Some(ISSUE_SIZE_LIFT_NEGATIVE) })
    );
}

#[test]
fn positive_constant_and_runtime_size_args_stay_clean() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut interner = Interner::new();
    let h = interner.intern("h");
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let positive = checker.types.intern_index(IndexExpr::Literal(3));
    let scalar = checker
        .types
        .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarParameter {
            parameter: 2,
        }));
    let runtime_witness = radix_types::add_index_constant(&mut checker.types, scalar, -1)
        .expect("runtime affine extent");
    checker.current_shape_call = Some(HirId(7));
    let params = magnitudo_params(&[h]);

    let mut shape_vars = FxHashMap::default();
    shape_vars.insert(h, positive);
    let positive_symbols =
        checker.capture_runtime_call_size_bindings(&params, &shape_vars, Span::default());
    checker.reject_negative_constant_size_args(
        &params,
        &shape_vars,
        &positive_symbols,
        Span::default(),
    );
    assert!(positive_symbols.is_empty());
    assert!(checker.errors.is_empty());

    shape_vars.insert(h, runtime_witness);
    let runtime_symbols =
        checker.capture_runtime_call_size_bindings(&params, &shape_vars, Span::default());
    checker.reject_negative_constant_size_args(
        &params,
        &shape_vars,
        &runtime_symbols,
        Span::default(),
    );
    assert!(runtime_symbols.contains(&h));
    assert!(checker.errors.is_empty());
    let binding = checker
        .shape_obligations
        .call_size_bindings
        .get(&HirId(7))
        .and_then(|rows| rows.first())
        .expect("runtime witness keeps the sidecar binding");
    assert_eq!(binding.witness, runtime_witness);
}
