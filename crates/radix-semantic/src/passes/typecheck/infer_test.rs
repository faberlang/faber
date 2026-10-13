use super::*;
use crate::{GenericParamDescriptor, GenericParamSort, TypeParamConstraint};
use radix_lexer::Symbol;
use radix_target::Target;

// F-009: common_type must preserve optionality when one branch is Option<T>
// and the other is T or nihil.
#[test]
fn common_type_preserves_optionality_on_join() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);

    let numerus = checker.numerus_type();
    let opt_numerus = checker.types.option(numerus);
    let nihil = checker.types.primitive(Primitive::Nihil);
    let vacuous_span = radix_lexer::Span::default();

    // Option<T> + T → Option<T> (not T)
    let joined = checker.common_type(opt_numerus, numerus, vacuous_span);
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Option(inner) if *inner == numerus),
        "F-009: common_type(Option<Numerus>, Numerus) must return Option<Numerus>, got {:?}",
        checker.types.get(resolved),
    );

    // T + Option<T> → Option<T>
    let joined = checker.common_type(numerus, opt_numerus, vacuous_span);
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Option(inner) if *inner == numerus),
        "F-009: common_type(Numerus, Option<Numerus>) must return Option<Numerus>, got {:?}",
        checker.types.get(resolved),
    );

    // Option<T> + nihil → Option<T> (not nihil)
    let joined = checker.common_type(opt_numerus, nihil, vacuous_span);
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Option(inner) if *inner == numerus),
        "F-009: common_type(Option<Numerus>, nihil) must return Option<Numerus>, got {:?}",
        checker.types.get(resolved),
    );

    // nihil + Option<T> → Option<T>
    let joined = checker.common_type(nihil, opt_numerus, vacuous_span);
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Option(inner) if *inner == numerus),
        "F-009: common_type(nihil, Option<Numerus>) must return Option<Numerus>, got {:?}",
        checker.types.get(resolved),
    );
}

// F-010: structural unify for Vector/Matrix/Sparsa/Atomic/Intervallum.
#[test]
fn unify_with_policy_handles_vector_matrix_sparsa_atomic_intervallum() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();

    // Create all type constructors before the checker borrows types.
    let f32_id = types.intern(Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32));
    let width_4 = types.intern_index(IndexExpr::Literal(4));
    let shape_2x2 = {
        let lit2 = types.intern_index(IndexExpr::Literal(2));
        types.intern_index(IndexExpr::Tuple(vec![lit2, lit2]))
    };
    let vec_a = types.vector_with_width(f32_id, width_4);
    let mat_a = types.matrix_with_shape(f32_id, shape_2x2);
    let sp_a = types.sparsa_with_shape(f32_id, shape_2x2);
    let atom_a = types.atomic(f32_id);
    let int_a = types.intervallum(f32_id);

    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let numerus = checker.numerus_type();
    let vacuous_span = radix_lexer::Span::default();

    // Create second set via checker.types
    let vec_b = checker.types.vector_with_width(numerus, width_4);
    let mat_b = checker.types.matrix_with_shape(numerus, shape_2x2);
    let sp_b = checker.types.sparsa_with_shape(numerus, shape_2x2);
    let atom_b = checker.types.atomic(numerus);
    let int_b = checker.types.intervallum(numerus);

    // Vector: unify element types, width must match
    let joined = checker.unify_join(vec_a, vec_b, vacuous_span, "test");
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Vector(_, _)),
        "F-010: unify_join(Vector<f32,4>, Vector<numerus,4>) must produce a Vector, got {:?}",
        checker.types.get(resolved),
    );

    // Matrix: unify element types, shape must match
    let joined = checker.unify_join(mat_a, mat_b, vacuous_span, "test");
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Matrix(_, _)),
        "F-010: unify_join(Matrix<f32,[2,2]>, Matrix<numerus,[2,2]>) must produce a Matrix, got {:?}",
        checker.types.get(resolved),
    );

    // Sparsa: unify element types, shape must match
    let joined = checker.unify_join(sp_a, sp_b, vacuous_span, "test");
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Sparsa(_, _)),
        "F-010: unify_join(Sparsa<f32,[2,2]>, Sparsa<numerus,[2,2]>) must produce a Sparsa, got {:?}",
        checker.types.get(resolved),
    );

    // Atomic: unify inner types
    let joined = checker.unify_join(atom_a, atom_b, vacuous_span, "test");
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Atomic(_)),
        "F-010: unify_join(Atomic<f32>, Atomic<numerus>) must produce an Atomic, got {:?}",
        checker.types.get(resolved),
    );

    // Intervallum: unify inner types
    let joined = checker.unify_join(int_a, int_b, vacuous_span, "test");
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Intervallum(_)),
        "F-010: unify_join(Intervallum<f32>, Intervallum<numerus>) must produce an Intervallum, got {:?}",
        checker.types.get(resolved),
    );
}

// F-012: function type join must treat params contravariantly under Join.
#[test]
fn function_type_join_uses_contravariant_params() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();

    let f32_id = types.intern(Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32));
    let fractus = types.primitive(Primitive::Fractus);
    let vacuum = types.primitive(Primitive::Vacuum);

    // f1: (f32) → void  (narrower param)
    let sig_a = FuncSig {
        type_params: vec![],
        params: vec![ParamType {
            ty: f32_id,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };
    let func_a = types.function(sig_a.clone());

    // f2: (fractus) → void  (wider param)
    let sig_b = FuncSig {
        type_params: vec![],
        params: vec![ParamType {
            ty: fractus,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };
    let func_b = types.function(sig_b);

    // f3: (f32) → void (same param types, join should succeed)
    // Pre-create before checker borrow so the join test below does not
    // double-borrow `types`.
    let func_c = types.function(FuncSig {
        type_params: vec![],
        params: vec![ParamType {
            ty: f32_id,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    });

    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let vacuous_span = radix_lexer::Span::default();

    // Sanity: f32 is assignable to fractus (widening), but not vice versa.
    let f32_resolved = checker.resolve_inferred_type(f32_id);
    let fractus_resolved = checker.resolve_inferred_type(fractus);
    assert!(
        !checker.types.is_assignable(fractus_resolved, f32_resolved),
        "F-012 precondition: fractus must not be assignable to f32 (narrowing)"
    );
    assert!(
        checker.types.is_assignable(f32_resolved, fractus_resolved),
        "F-012 precondition: f32 must be assignable to fractus (widening)"
    );

    // Under the old covariant Join, (f32)→void and (fractus)→void would
    // unsoundly join to (fractus)→void (wider param). Under the fixed
    // contravariant Join using Assign for params, incompatible param types
    // cause the join to fail because Assign checks is_assignable(param_b, param_a)
    // = is_assignable(fractus, f32) = false.
    let joined = checker.unify_join(func_a, func_b, vacuous_span, "test");
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Error),
        "F-012: function join with incompatible param types must fail (was unsoundly covariant); got {:?}",
        checker.types.get(resolved),
    );

    // When params are the same, join should succeed contravariantly.
    let joined = checker.unify_join(func_a, func_c, vacuous_span, "test");
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        !matches!(checker.types.get(resolved), Type::Error),
        "F-012: function join with same param types must succeed",
    );
}

// PRP-076: occurs_in must traverse type_param_constraints.
#[test]
fn occurs_in_detects_var_in_type_param_constraints() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);

    let infer = checker.fresh_infer();
    let numerus = checker.numerus_type();

    let sig = FuncSig {
        type_params: vec![GenericParamDescriptor {
            name: Symbol(0),
            sort: GenericParamSort::Typus,
            constraint: TypeParamConstraint::OneOf(vec![infer, numerus]),
            default: None,
        }],
        params: vec![],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    };
    let func_ty = checker.types.function(sig);

    assert!(
        checker.occurs_in(InferVar(0), func_ty),
        "occurs_in must detect var inside type_param_constraints::OneOf",
    );
}

#[test]
fn occurs_in_ignores_any_constraint() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);

    let numerus = checker.numerus_type();

    // Create a FuncSig with TypeParamConstraint::Any (no TypeId references).
    let sig = FuncSig {
        type_params: vec![GenericParamDescriptor {
            name: Symbol(0),
            sort: GenericParamSort::Typus,
            constraint: TypeParamConstraint::Any,
            default: None,
        }],
        params: vec![],
        ret: numerus,
        err: None,
        is_async: false,
        is_generator: false,
    };
    let func_ty = checker.types.function(sig);

    // var 999 does not occur anywhere — confirms Any never produces a false positive.
    assert!(
        !checker.occurs_in(InferVar(999), func_ty),
        "occurs_in must not false-positive through TypeParamConstraint::Any",
    );
}

// A-198: every collection shape that carries a type child must be walked by
// the occurs check, or `T = queue<T>` binds a cyclic substitution.
#[test]
fn occurs_in_walks_queue_stack_ledger_bounded_and_intersection() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);

    let infer = checker.fresh_infer();
    let numerus = checker.numerus_type();
    let capacity = checker
        .types
        .intern_index(radix_types::IndexExpr::Literal(4));

    let shapes = [
        ("queue", checker.types.queue(infer)),
        ("stack", checker.types.stack(infer)),
        (
            "bounded lista",
            checker.types.bounded_array(infer, capacity),
        ),
        (
            "bounded queue",
            checker.types.bounded_queue(infer, capacity),
        ),
        (
            "bounded stack",
            checker.types.bounded_stack(infer, capacity),
        ),
        ("ledger key", checker.types.ledger(infer, numerus)),
        ("ledger value", checker.types.ledger(numerus, infer)),
        (
            "intersection",
            checker
                .types
                .intern(Type::Intersection(vec![numerus, infer])),
        ),
    ];

    for (label, ty) in shapes {
        assert!(
            checker.occurs_in(InferVar(0), ty),
            "occurs_in must detect the var inside {label}",
        );
    }
}

// A-197: the expression dispatcher saves the pending failable-call conversion
// on entry and must hand it back on every exit, including the named-error
// returns of the inferred-constructor desugars.
#[test]
fn check_expr_early_return_restores_pending_conversion() {
    use radix_hir::{HirExpression, HirExpressionKind, HirId};

    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();

    // `_ [ … ]` with no expected type: the bracket-literal desugar names the
    // error and settles the node before the ordinary check runs.
    let hole = checker.fresh_infer();
    let mut expr = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Verte {
            source: Box::new(HirExpression {
                id: HirId(2),
                kind: HirExpressionKind::Array(Vec::new()),
                ty: None,
                span,
            }),
            target: hole,
            entries: None,
        },
        ty: None,
        span,
    };

    checker.pending_conversion = Some(DefId(7));
    let ty = checker.check_expr_with_expected(&mut expr, None);

    assert_eq!(ty, checker.error_type, "the hole has no expected type");
    assert_eq!(
        checker.pending_conversion,
        Some(DefId(7)),
        "an early return must restore the saved pending conversion",
    );
}

// The Applied arm delegates its paired-argument walk to `unify_applied_args`:
// type slots unify structurally, size slots stay equal at an absent boundary,
// and a kind mismatch fails the join with the caller's issue.
#[test]
fn unify_applied_args_pairs_type_and_size_slots() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let span = radix_lexer::Span::default();

    let base = checker.types.intern(Type::Struct(DefId(7)));
    let numerus = checker.numerus_type();
    let fractus = checker.fractus_type();
    let width = checker.types.intern_index(IndexExpr::Literal(4));

    let applied = |checker: &mut TypeChecker<'_>, type_arg| {
        checker.types.intern(Type::Applied(
            base,
            vec![type_arg, radix_types::AppliedArg::Size(width)],
        ))
    };
    let left = applied(&mut checker, radix_types::AppliedArg::Type(numerus));
    let right = applied(&mut checker, radix_types::AppliedArg::Type(fractus));

    let joined = checker.unify_join(left, right, span, "test");
    assert!(checker.errors.is_empty(), "{:?}", checker.errors);
    let resolved = checker.resolve_inferred_type(joined);
    assert!(
        matches!(checker.types.get(resolved), Type::Applied(_, args) if args.len() == 2),
        "expected paired Applied, got {:?}",
        checker.types.get(resolved),
    );

    // Kind mismatch: a `Size` slot paired with a `Type` slot reports `issue`.
    let size_only = checker.types.intern(Type::Applied(
        base,
        vec![radix_types::AppliedArg::Size(width)],
    ));
    let type_only = checker.types.intern(Type::Applied(
        base,
        vec![radix_types::AppliedArg::Type(numerus)],
    ));
    let failed = checker.unify_join(size_only, type_only, span, "test");
    assert!(!checker.errors.is_empty());
    assert_eq!(failed, checker.error_type);
}
