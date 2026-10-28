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

/// A-03: wrappers for the container variants whose element position carries a
/// type (or a figura leaf) and that both generic traversals must rebuild, the
/// way they rebuild the `lista<T>` twin.
fn unbounded_wrappers() -> Vec<(&'static str, fn(&mut TypeTable, TypeId) -> TypeId)> {
    vec![
        ("lista", |types, inner| types.array(inner)),
        ("queue", |types, inner| types.queue(inner)),
        ("stack", |types, inner| types.stack(inner)),
        ("atomic", |types, inner| types.atomic(inner)),
        ("ledger value", |types, inner| {
            let key = types.primitive(Primitive::Textus);
            types.ledger(key, inner)
        }),
        ("ledger key", |types, inner| {
            let value = types.primitive(Primitive::Textus);
            types.ledger(inner, value)
        }),
        ("intersection", |types, inner| {
            let other = types.primitive(Primitive::Textus);
            types.intern(Type::Intersection(vec![inner, other]))
        }),
        ("record", |types, inner| {
            types.record([(radix_lexer::Symbol(900), inner)])
        }),
    ]
}

fn bounded_wrappers() -> Vec<(&'static str, fn(&mut TypeTable, TypeId, IndexId) -> TypeId)> {
    vec![
        ("bounded lista", |types, element, capacity| {
            types.bounded_array(element, capacity)
        }),
        ("bounded queue", |types, element, capacity| {
            types.bounded_queue(element, capacity)
        }),
        ("bounded stack", |types, element, capacity| {
            types.bounded_stack(element, capacity)
        }),
    ]
}

#[test]
fn substitute_type_params_rebuilds_every_container_variant() {
    for (name, wrap) in unbounded_wrappers() {
        let mut types = TypeTable::new();
        let resolver = Resolver::new();
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
        let param = checker.types.intern(Type::Param(t));
        let concrete = checker.types.primitive(Primitive::Numerus);
        let declared = wrap(checker.types, param);
        let expected = wrap(checker.types, concrete);
        let mut vars = FxHashMap::default();
        vars.insert(t, concrete);

        let instantiated = checker.substitute_type_params(declared, &mut vars, None);

        assert_eq!(
            instantiated,
            expected,
            "{name}<T> must substitute T := numerus; got {:?}",
            checker.types.get(instantiated)
        );
    }
}

#[test]
fn substitute_type_params_substitutes_bounded_capacity_and_element() {
    for (name, wrap) in bounded_wrappers() {
        let mut types = TypeTable::new();
        let resolver = Resolver::new();
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let n = interner.intern("N");
        let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
        let param = checker.types.intern(Type::Param(t));
        let capacity_param = checker.types.intern_index(IndexExpr::Param(n));
        let concrete = checker.types.primitive(Primitive::Numerus);
        let four = checker.types.intern_index(IndexExpr::Literal(4));
        let declared = wrap(checker.types, param, capacity_param);
        let expected = wrap(checker.types, concrete, four);
        let mut vars = FxHashMap::default();
        vars.insert(t, concrete);
        let mut shape_vars = FxHashMap::default();
        shape_vars.insert(n, four);

        let instantiated =
            checker.substitute_type_params(declared, &mut vars, Some(&mut shape_vars));

        assert_eq!(
            instantiated,
            expected,
            "{name}<T, N> must substitute T := numerus, N := 4; got {:?}",
            checker.types.get(instantiated)
        );
    }
}

#[test]
fn freshen_anonymous_type_holes_reaches_every_container_variant() {
    for (name, wrap) in unbounded_wrappers() {
        let mut types = TypeTable::new();
        let resolver = Resolver::new();
        let interner = Interner::new();
        let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
        let element = checker.types.primitive(Primitive::Fractus);
        let hole = checker.fresh_index_hole();
        let figura = checker.types.tensor_with_shape(element, hole);
        let declared = wrap(checker.types, figura);
        let mut holes = FxHashMap::default();

        let freshened = checker.freshen_anonymous_type_holes(declared, &mut holes);

        assert_eq!(
            holes.len(),
            1,
            "{name}<tensor<_>> must reach the extent hole; got {:?}",
            checker.types.get(freshened)
        );
        assert_ne!(
            freshened, declared,
            "{name}<tensor<_>> must get a per-call hole instead of the declared one"
        );
    }
}

#[test]
fn freshen_anonymous_type_holes_reaches_bounded_capacity_and_element() {
    for (name, wrap) in bounded_wrappers() {
        let mut types = TypeTable::new();
        let resolver = Resolver::new();
        let interner = Interner::new();
        let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
        let element = checker.types.primitive(Primitive::Fractus);
        let capacity = checker.fresh_index_hole();
        let extent = checker.fresh_index_hole();
        let figura = checker.types.tensor_with_shape(element, extent);
        let declared = wrap(checker.types, figura, capacity);
        let mut holes = FxHashMap::default();

        let freshened = checker.freshen_anonymous_type_holes(declared, &mut holes);

        assert_eq!(
            holes.len(),
            2,
            "{name}<tensor<_>, _> must freshen capacity and element extent; got {:?}",
            checker.types.get(freshened)
        );
        assert_ne!(freshened, declared, "{name} kept its declared holes");
    }
}

/// A-03 at the source level: the call-site instantiation of `queue<T>` must
/// bind `T` like the `lista<T>` twin instead of comparing a rigid `T`.
#[test]
fn generic_call_binds_type_param_through_queue_and_stack() {
    crate::test_support::assert_accepts(
        r#"functio peek_lista<T>(lista<T> q) → numerus {
    redde 0
}

functio peek_queue<T>(queue<T> q) → numerus {
    redde 0
}

functio peek_stack<T>(stack<T> s) → numerus {
    redde 0
}

functio peek_bounded_queue<T>(queue<T, magnitudo 4> q) → numerus {
    redde 0
}

functio peek_bounded_stack<T>(stack<T, magnitudo 4> s) → numerus {
    redde 0
}

functio caller(lista<numerus> l, queue<numerus> q, stack<numerus> s, queue<numerus, magnitudo 4> bq, stack<numerus, magnitudo 4> bs) → numerus {
    sit numerus a ← peek_lista<numerus>(l)
    sit numerus b ← peek_queue<numerus>(q)
    sit numerus c ← peek_stack<numerus>(s)
    sit numerus d ← peek_bounded_queue<numerus>(bq)
    sit numerus e ← peek_bounded_stack<numerus>(bs)
    redde a + b + c + d + e
}"#,
    );
}
