use super::*;

#[test]
fn dynamic_call_shape_mismatch_becomes_a_call_obligation() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let call = HirId(41);
    let actual_dim = checker
        .types
        .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(7))));
    let expected_dim = checker
        .types
        .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(8))));
    let actual_shape = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![actual_dim]));
    let expected_shape = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![expected_dim]));
    checker.current_shape_call = Some(call);
    checker.current_shape_argument = Some(1);

    let unified = checker
        .unify_shape_at_boundary(actual_shape, expected_shape, radix_lexer::Span::default())
        .expect("call context should use the runtime shape path")
        .expect("dynamic extents should be deferred to the call boundary");

    assert!(matches!(
        checker.types.get_index(unified),
        IndexExpr::Tuple(_)
    ));
    let relations = checker
        .shape_obligations
        .calls
        .get(&call)
        .expect("call relation should be keyed by the outer HIR call");
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].boundary, ShapeRelationBoundary::Call);
    assert!(matches!(
        checker.types.get_index(relations[0].left),
        IndexExpr::Runtime(RuntimeExtentId::CallArgumentAxis {
            argument: 1,
            axis: 0
        })
    ));
    assert_eq!(relations[0].right, expected_dim);
}

#[test]
fn dynamic_return_shape_mismatch_becomes_a_return_obligation() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let function = DefId(8);
    let expression = HirId(45);
    let actual_dim = checker
        .types
        .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(9))));
    let expected_dim =
        checker
            .types
            .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(
                10,
            ))));
    let actual = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![actual_dim]));
    let expected = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![expected_dim]));
    checker.current_function_def = Some(function);
    checker.current_shape_return_expr = Some(expression);

    checker
        .unify_shape_at_boundary(actual, expected, radix_lexer::Span::default())
        .expect("return context should use the runtime shape path")
        .expect("dynamic return shapes should be checked before return");

    let relations = checker
        .shape_obligations
        .functions
        .get(&function)
        .expect("return relation should be stored with its function");
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].boundary, ShapeRelationBoundary::FunctionReturn);
    assert!(matches!(
        checker.types.get_index(relations[0].left),
        IndexExpr::Runtime(RuntimeExtentId::ExpressionAxis {
            expression: 45,
            axis: 0
        })
    ));
    assert_eq!(relations[0].right, expected_dim);
}

#[test]
fn static_call_shape_mismatch_remains_a_type_error() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    checker.current_shape_call = Some(HirId(42));
    checker.current_shape_argument = Some(0);
    let actual_dim = checker.types.intern_index(IndexExpr::Literal(2));
    let expected_dim = checker.types.intern_index(IndexExpr::Literal(3));
    let actual = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![actual_dim]));
    let expected = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![expected_dim]));

    assert!(
        checker
            .unify_shape_at_boundary(actual, expected, radix_lexer::Span::default())
            .expect("call context should use the runtime shape path")
            .is_err()
    );
    assert!(checker.shape_obligations.calls.is_empty());
    assert_eq!(checker.errors.len(), 1);
}

#[test]
fn affine_call_shape_keeps_actual_dimension_after_solving_generic_base() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    checker.current_shape_call = Some(HirId(46));
    checker.current_shape_argument = Some(0);
    let size = checker.fresh_index_hole();
    let expected_dim = radix_types::add_index_constant(&mut checker.types, size, -1)
        .expect("affine generic dimension");
    let actual_dim = checker.types.intern_index(IndexExpr::Literal(3));
    let actual = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![actual_dim]));
    let expected = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![expected_dim]));

    let unified = checker
        .unify_shape_at_boundary(actual, expected, radix_lexer::Span::default())
        .expect("call context should use the runtime shape path")
        .expect("N - 1 should infer N from a concrete actual shape");
    let IndexExpr::Tuple(dims) = checker.types.get_index(unified) else {
        panic!("expected a rank-one shape");
    };
    assert!(matches!(
        checker.types.get_index(dims[0]),
        IndexExpr::Literal(3)
    ));
    let inferred_size = checker.canonicalize_index(size);
    assert!(matches!(
        checker.types.get_index(inferred_size),
        IndexExpr::Literal(4)
    ));
}

#[test]
fn concrete_inexact_generic_division_rejects_instead_of_becoming_runtime_relation() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let size = checker.fresh_index_hole();
    let five = checker.types.intern_index(IndexExpr::Literal(5));
    {
        let mut unifier = checker.index_infer.unifier(checker.types);
        unifier
            .unify(size, five)
            .expect("first argument binds D = 5");
    }
    let divided = radix_types::divide_index_exact(checker.types, size, 2)
        .expect("symbolic exact-divide term");
    let actual_dim = checker.types.intern_index(IndexExpr::Literal(2));
    let actual = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![actual_dim]));
    let expected = checker.types.intern_index(IndexExpr::Tuple(vec![divided]));
    checker.current_shape_call = Some(HirId(47));
    checker.current_shape_argument = Some(1);

    assert!(
        checker
            .unify_shape_at_boundary(actual, expected, radix_lexer::Span::default())
            .expect("call context should use the runtime shape path")
            .is_err()
    );
    assert!(checker.shape_obligations.calls.is_empty());
    assert!(checker.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "incompatible_tensor_index")
    }));
}

#[test]
fn same_runtime_local_extent_keeps_declared_tensor_shape_consistent() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let n = checker
        .types
        .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(
            21,
        ))));
    let two = checker.types.intern_index(IndexExpr::Literal(2));
    let shape = checker.types.intern_index(IndexExpr::Tuple(vec![n, two]));
    let element = checker
        .types
        .sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let tensor = checker.types.tensor_with_shape(element, shape);

    let unified = checker.unify_carrier_assign(
        tensor,
        tensor,
        radix_lexer::Span::default(),
        "initializer_type_mismatch",
    );

    assert!(matches!(checker.types.get(unified), Type::Tensor(_, _)));
    assert!(checker.errors.is_empty());
}

#[test]
fn unrelated_runtime_local_initializer_is_rejected_without_false_shape_fact() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let actual_n = checker
        .types
        .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(
            22,
        ))));
    let expected_n =
        checker
            .types
            .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(
                23,
            ))));
    let two = checker.types.intern_index(IndexExpr::Literal(2));
    let actual_shape = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![actual_n, two]));
    let expected_shape = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![expected_n, two]));
    let element = checker
        .types
        .sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let actual = checker.types.tensor_with_shape(element, actual_shape);
    let expected = checker.types.tensor_with_shape(element, expected_shape);

    let unified = checker.unify_carrier_assign(
        actual,
        expected,
        radix_lexer::Span::default(),
        "initializer_type_mismatch",
    );

    assert!(matches!(checker.types.get(unified), Type::Error));
    assert!(!checker.errors.is_empty());
    assert!(checker.shape_obligations.calls.is_empty());
    assert!(checker.shape_obligations.functions.is_empty());
}

#[test]
fn runtime_tensor_constructor_and_annotation_share_the_same_local_extent() {
    let source = r#"
fn runtime_extent(int value) → int { return value }

main {
    const int n ← runtime_extent(2)
    const list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    const tf32[] seed ← empty
    const tf32[n, 2] tensor ← seed.from_flat(flat, [n, 2])
    const tf32[n, 2] same_shape ← tensor
    print same_shape.shape()
}
"#;
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("English locale pack"),
    );
    let result = crate::test_support::analyze_source_with_config(source, config);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn unrelated_runtime_local_annotation_is_rejected() {
    let source = r#"
fn runtime_extent(int value) → int { return value }

main {
    const int n ← runtime_extent(2)
    const int m ← runtime_extent(3)
    const list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    const tf32[] seed ← empty
    const tf32[m, 2] tensor_m ← seed.from_flat(flat, [m, 2])
    const tf32[n, 2] invalid ← tensor_m
    print invalid.shape()
}
"#;
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("English locale pack"),
    );
    let result = crate::test_support::analyze_source_with_config(source, config);
    assert!(
        !result.is_ok(),
        "source with unrelated extents was accepted"
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.is_error() && diagnostic.issue() == Some("incompatible_tensor_index")
        }),
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn whole_shape_hole_does_not_infer_an_unknown_rank_at_a_call() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    checker.current_shape_call = Some(HirId(43));
    checker.current_shape_argument = Some(0);
    let unknown_rank = checker.fresh_index_hole();
    let three = checker.types.intern_index(IndexExpr::Literal(3));
    let rank_one = checker.types.intern_index(IndexExpr::Tuple(vec![three]));

    assert!(
        checker
            .unify_shape_at_boundary(unknown_rank, rank_one, radix_lexer::Span::default())
            .expect("call context should enforce the rank rule")
            .is_err()
    );
    assert!(checker.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "cannot_infer_tensor_shape")
    }));
    assert!(checker.shape_obligations.calls.is_empty());
}

#[test]
fn runtime_size_binding_uses_generic_ordinal_and_keeps_runtime_source() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = radix_lexer::Interner::new();
    let n = interner.intern("N");
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let scalar = checker
        .types
        .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarParameter {
            parameter: 2,
        }));
    let witness = radix_types::add_index_constant(&mut checker.types, scalar, -1)
        .expect("runtime affine extent");
    checker.current_shape_call = Some(HirId(44));
    let params = [radix_types::GenericParamDescriptor {
        name: n,
        sort: radix_types::GenericParamSort::Magnitudo,
        constraint: radix_types::TypeParamConstraint::Any,
        default: None,
    }];
    let mut shape_vars = FxHashMap::default();
    shape_vars.insert(n, witness);

    let runtime_symbols = checker.capture_runtime_call_size_bindings(
        &params,
        &shape_vars,
        radix_lexer::Span::default(),
    );

    assert!(runtime_symbols.contains(&n));
    let binding = checker
        .shape_obligations
        .call_size_bindings
        .get(&HirId(44))
        .and_then(|rows| rows.first())
        .expect("dynamic size witness should be retained for backend lowering");
    assert_eq!(binding.parameter, 0);
    assert_eq!(binding.witness, witness);
    assert!(matches!(
        checker.types.get_index(binding.witness),
        IndexExpr::Affine {
            base,
            offset: -1
        } if matches!(checker.types.get_index(*base), IndexExpr::Runtime(
            RuntimeExtentId::ScalarParameter { parameter: 2 }
        ))
    ));
}

#[test]
fn affine_record_field_size_binding_uses_formal_axes_and_checks_repeats() {
    let resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = radix_lexer::Interner::new();
    let n = interner.intern("N");
    let grid = interner.intern("grid");
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let element = checker
        .types
        .sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let param_n = checker.types.intern_index(IndexExpr::Param(n));
    let shape_n_minus_one = radix_types::add_index_constant(&mut checker.types, param_n, -1)
        .expect("symbolic affine dimension");
    let shape = checker
        .types
        .intern_index(IndexExpr::Tuple(vec![shape_n_minus_one]));
    let tensor = checker.types.tensor_with_shape(element, shape);
    let record = checker.types.record(vec![(grid, tensor)]);
    let parameter = |def_id, name| HirParam {
        def_id,
        name,
        ty: record,
        mode: radix_hir::HirParamMode::In,
        optional: false,
        sponte: false,
        default: None,
        span: radix_lexer::Span::default(),
    };
    let generic = radix_hir::HirTypeParam {
        def_id: DefId(9),
        kind: radix_syntax::GenericParamKind::Magnitudo,
        name: n,
        span: radix_lexer::Span::default(),
        constraint: radix_hir::HirTypeParamConstraint::Any,
        default: None,
    };
    checker.current_function_def = Some(DefId(10));
    checker.install_function_shape_sources(
        &[generic],
        &[parameter(DefId(11), n), parameter(DefId(12), n)],
    );

    let size_binding = checker
        .shape_obligations
        .function_size_bindings
        .get(&DefId(10))
        .and_then(|rows| rows.first())
        .expect("generic size should be witnessed by the first formal axis");
    assert_eq!(size_binding.parameter, 0);
    let IndexExpr::Affine { base, offset: 1 } = checker.types.get_index(size_binding.witness)
    else {
        panic!("generic size witness should add one to the first field axis");
    };
    assert!(matches!(
        checker.types.get_index(*base),
        IndexExpr::Runtime(RuntimeExtentId::FunctionParameterFieldAxis {
            parameter: 0,
            fields,
            axis: 0
        }) if fields.as_slice() == &[grid]
    ));
    let relations = checker
        .shape_obligations
        .functions
        .get(&DefId(10))
        .expect("repeated size symbol should create an entry relation");
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].boundary, ShapeRelationBoundary::FunctionEntry);
    assert!(matches!(
        checker.types.get_index(relations[0].left),
        IndexExpr::Runtime(RuntimeExtentId::FunctionParameterFieldAxis {
            parameter: 1,
            fields,
            axis: 0
        }) if fields.as_slice() == &[grid]
    ));
}
