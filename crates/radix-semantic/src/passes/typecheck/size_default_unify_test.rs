use super::*;
use radix_lexer::{Interner, Span};
use radix_target::Target;

/// S00-4: a magnitudo default that cannot unify with the already-inferred
/// hole value must report a diagnostic, never return the default as if it
/// bound. `constrain_index_equal` stands in for any inference source that
/// substituted the hole before the default bind runs.
#[test]
fn size_default_conflicting_with_inferred_value_reports() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let hole = checker.fresh_index_hole();
    let inferred = checker.types.intern_index(IndexExpr::Literal(3));
    checker.constrain_index_equal(hole, inferred, Span::default());
    assert!(
        checker.errors.is_empty(),
        "setup must not emit diagnostics: {:?}",
        checker.errors
    );

    let bound = checker.bind_size_param_default(hole, 4, Span::default());

    assert!(
        bound.is_none(),
        "failed default bind must not yield a witness id"
    );
    assert!(
        checker.errors.iter().any(|error| {
            error.kind == SemanticErrorKind::TypeMismatch
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "incompatible_tensor_index")
        }),
        "expected incompatible_tensor_index diagnostic, got: {:?}",
        checker.errors
    );
}

#[test]
fn affine_substitution_cancels_before_natural_validation() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut interner = Interner::new();
    let n = interner.intern("N");
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let parameter = checker.types.intern_index(IndexExpr::Param(n));
    let minus_one = radix_types::compose_index_affine_constant(&mut checker.types, parameter, -1)
        .expect("symbolic offset");
    let canceled = radix_types::compose_index_affine_constant(&mut checker.types, minus_one, 1)
        .expect("canceled affine offset");
    let zero = checker.types.intern_index(IndexExpr::Literal(0));
    let mut substitutions = FxHashMap::default();
    substitutions.insert(n, zero);

    let instantiated = checker.substitute_index_params(canceled, Some(&mut substitutions));
    assert_eq!(
        radix_types::evaluate_index_dimension(instantiated, &checker.types),
        Ok(Some(0))
    );
}

#[test]
fn exact_division_substitution_preserves_non_integral_failure() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut interner = Interner::new();
    let d = interner.intern("D");
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let parameter = checker.types.intern_index(IndexExpr::Param(d));
    let divided = radix_types::divide_index_exact(&mut checker.types, parameter, 2)
        .expect("symbolic exact division");
    let five = checker.types.intern_index(IndexExpr::Literal(5));
    let mut substitutions = FxHashMap::default();
    substitutions.insert(d, five);

    let instantiated = checker.substitute_index_params(divided, Some(&mut substitutions));
    assert_eq!(
        radix_types::evaluate_index_dimension(instantiated, &checker.types),
        Err(radix_types::IndexArithmeticError::NonIntegral)
    );
    let f32_ty = checker
        .types
        .sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let tensor = checker.types.tensor_with_shape(f32_ty, instantiated);
    assert_eq!(
        checker.first_index_arithmetic_error_in_type(tensor),
        Some(radix_types::IndexArithmeticError::NonIntegral)
    );
}
