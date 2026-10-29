use super::*;
use radix_types::{DefId, Primitive, RuntimeExtentId, SemanticParamMode};

/// Build a self-referential Alias: `Alias(_, TypeId(n))` interned at index `n`.
fn self_cyclic_alias(types: &mut TypeTable, def: u32) -> TypeId {
    let next = u32::try_from(types.type_count()).expect("type table fits u32");
    types.intern(Type::Alias(DefId(def), TypeId(next)))
}

/// Two-node Alias cycle: A → B → A.
fn mutual_cyclic_aliases(types: &mut TypeTable) -> (TypeId, TypeId) {
    let n = u32::try_from(types.type_count()).expect("type table fits u32");
    let a = types.intern(Type::Alias(DefId(1), TypeId(n + 1)));
    let b = types.intern(Type::Alias(DefId(2), TypeId(n)));
    assert_eq!(a, TypeId(n));
    assert_eq!(b, TypeId(n + 1));
    (a, b)
}

#[test]
fn alias_cycle_self_terminates() {
    let mut types = TypeTable::new();
    let cyclic = self_cyclic_alias(&mut types, 10);
    // Pure cyclic Alias has no Param, so bind_generic_type_witness would
    // early-return true via type_contains_param. Pair the cycle with a
    // Param so try_bind walks the Alias arm and hits the seen-set guard.
    let param = types.intern(Type::Param(Symbol(99)));
    let expected = types.map(cyclic, param);
    let numerus = types.primitive(Primitive::Numerus);
    let actual = types.map(numerus, numerus);
    let mut bindings = MirGenericBindings::default();

    // Must return false rather than overflow the stack.
    assert!(!bind_generic_type_witness(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
    ));
    assert!(bindings.is_empty());
}

#[test]
fn alias_cycle_mutual_terminates() {
    let mut types = TypeTable::new();
    let (a, _b) = mutual_cyclic_aliases(&mut types);
    let param = types.intern(Type::Param(Symbol(98)));
    let expected = types.map(a, param);
    let numerus = types.primitive(Primitive::Numerus);
    let actual = types.map(numerus, numerus);
    let mut bindings = MirGenericBindings::default();

    assert!(!bind_generic_type_witness(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
    ));
    assert!(bindings.is_empty());
}

#[test]
fn acyclic_alias_binds_param() {
    let mut types = TypeTable::new();
    let param = types.intern(Type::Param(Symbol(42)));
    let alias = types.intern(Type::Alias(DefId(7), param));
    let numerus = types.primitive(Primitive::Numerus);
    let mut bindings = MirGenericBindings::default();

    assert!(bind_generic_type_witness(
        &types,
        MirType::semantic(alias),
        MirType::semantic(numerus),
        &mut bindings,
    ));
    assert_eq!(
        bindings.get(&Symbol(42)).copied().map(MirType::semantic_id),
        Some(numerus)
    );
}

#[test]
fn shared_param_in_map_still_binds() {
    // Regression: path-local Alias seen-set must not break DAG sharing of
    // the same Param TypeId in map key and value positions.
    let mut types = TypeTable::new();
    let param = types.intern(Type::Param(Symbol(1)));
    let map_expected = types.map(param, param);
    let numerus = types.primitive(Primitive::Numerus);
    let map_actual = types.map(numerus, numerus);
    let mut bindings = MirGenericBindings::default();

    assert!(bind_generic_type_witness(
        &types,
        MirType::semantic(map_expected),
        MirType::semantic(map_actual),
        &mut bindings,
    ));
    assert_eq!(
        bindings.get(&Symbol(1)).copied().map(MirType::semantic_id),
        Some(numerus)
    );
}

#[test]
fn shared_alias_in_map_still_binds() {
    let mut types = TypeTable::new();
    let param = types.intern(Type::Param(Symbol(3)));
    let alias = types.intern(Type::Alias(DefId(9), param));
    let map_expected = types.map(alias, alias);
    let textus = types.primitive(Primitive::Textus);
    let map_actual = types.map(textus, textus);
    let mut bindings = MirGenericBindings::default();

    assert!(bind_generic_type_witness(
        &types,
        MirType::semantic(map_expected),
        MirType::semantic(map_actual),
        &mut bindings,
    ));
    assert_eq!(
        bindings.get(&Symbol(3)).copied().map(MirType::semantic_id),
        Some(textus)
    );
}

#[test]
fn union_backtracking_binds_without_clone() {
    let mut types = TypeTable::new();
    let t = types.intern(Type::Param(Symbol(10)));
    let u = types.intern(Type::Param(Symbol(11)));
    let v = types.intern(Type::Param(Symbol(12)));
    let expected = types.intern(Type::Union(vec![t, u, v]));

    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let bivalens = types.primitive(Primitive::Bivalens);
    // Permuted actual order forces backtracking past first candidates.
    let actual = types.intern(Type::Union(vec![bivalens, textus, numerus]));

    let mut bindings = MirGenericBindings::default();
    assert!(bind_generic_type_witness(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
    ));
    assert_eq!(bindings.len(), 3);
    assert_eq!(
        bindings.get(&Symbol(10)).copied().map(MirType::semantic_id),
        Some(textus)
    );
    assert_eq!(
        bindings.get(&Symbol(11)).copied().map(MirType::semantic_id),
        Some(numerus)
    );
    assert_eq!(
        bindings.get(&Symbol(12)).copied().map(MirType::semantic_id),
        Some(bivalens)
    );
}

#[test]
fn union_mismatch_undoes_partial_bindings() {
    let mut types = TypeTable::new();
    let t = types.intern(Type::Param(Symbol(20)));
    let numerus = types.primitive(Primitive::Numerus);
    let expected = types.intern(Type::Union(vec![t, numerus]));

    let textus = types.primitive(Primitive::Textus);
    let fractus = types.primitive(Primitive::Fractus);
    let actual = types.intern(Type::Union(vec![textus, fractus]));

    let mut bindings = MirGenericBindings::default();
    assert!(!bind_generic_type_witness(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
    ));
    // Failed candidate must not leak Param bindings into the caller's map.
    assert!(bindings.is_empty());
}

#[test]
fn union_backtracking_bounded_n8() {
    // N=8 → up to 8! = 40320 permutation paths. Without per-candidate
    // clone this must terminate quickly with one shared bindings map.
    let mut types = TypeTable::new();
    let params: Vec<TypeId> = (0..8)
        .map(|i| types.intern(Type::Param(Symbol(100 + i))))
        .collect();
    let expected = types.intern(Type::Union(params.clone()));

    let primitives = [
        Primitive::Numerus,
        Primitive::Textus,
        Primitive::Bivalens,
        Primitive::Fractus,
        Primitive::Octeti,
        Primitive::Ascii,
        Primitive::Instans,
        Primitive::Json,
    ];
    // Reverse order so the first expected member matches last actual.
    let actual_members: Vec<TypeId> = primitives
        .iter()
        .rev()
        .map(|p| types.primitive(*p))
        .collect();
    let actual = types.intern(Type::Union(actual_members));

    let mut bindings = MirGenericBindings::default();
    assert!(bind_generic_type_witness(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
    ));
    assert_eq!(bindings.len(), 8);
}

#[test]
fn promissum_failable_instantiation_finds_interned() {
    // Regression: generic instantiation of an already-interned
    // PromissumFailable must resolve through ExistingTypeIndex. Without the
    // type_matches_candidate arm (same latent gap AsyncCursor had), the
    // find falls through and reports "instantiated generic type is missing
    // semantic metadata" even though the concrete type is interned.
    let mut types = TypeTable::new();
    let success = types.primitive(Primitive::Numerus);
    let alternate = types.primitive(Primitive::Textus);

    // Concrete promissum-failable already interned in the module.
    let interned = types.intern(Type::PromissumFailable(success, alternate));

    // Generic shape `promissum-failable<T, U>`.
    let success_param = types.intern(Type::Param(Symbol(50)));
    let alternate_param = types.intern(Type::Param(Symbol(51)));
    let generic = types.intern(Type::PromissumFailable(success_param, alternate_param));

    let mut bindings = MirGenericBindings::default();
    bindings.insert(Symbol(50), MirType::semantic(success));
    bindings.insert(Symbol(51), MirType::semantic(alternate));

    let resolved = instantiate_existing_generic_type(
        &types,
        MirType::semantic(generic),
        &bindings,
        Span::default(),
    )
    .expect("generic PromissumFailable instantiation must resolve against the interned type");
    assert_eq!(resolved.semantic_id(), interned);
}

#[test]
fn generic_failable_function_type_instantiates_with_error_half() {
    // Regression: `fn<T>(string) → T ⇥ E` must keep its error half through
    // generic instantiation. Only the ret type is generic here; dropping
    // `err` would silently erase the call's failure channel.
    let mut types = TypeTable::new();
    let t = types.intern(Type::Param(Symbol(60)));
    let string = types.primitive(Primitive::Textus);
    let err = types.primitive(Primitive::Numerus);

    let generic = types.intern(Type::Func(FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: string,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: t,
        err: Some(err),
        is_async: false,
        is_generator: false,
    }));

    // Read-only path: the concrete signature is already interned.
    let concrete = types.intern(Type::Func(FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: string,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: string,
        err: Some(err),
        is_async: false,
        is_generator: false,
    }));

    let mut bindings = MirGenericBindings::default();
    bindings.insert(Symbol(60), MirType::semantic(string));

    let resolved = instantiate_existing_generic_type(
        &types,
        MirType::semantic(generic),
        &bindings,
        Span::default(),
    )
    .expect("generic failable function instantiation must resolve against the interned type");
    assert_eq!(resolved.semantic_id(), concrete);
    match types.get(resolved.semantic_id()) {
        Type::Func(sig) => assert_eq!(sig.err, Some(err)),
        other => panic!("expected Func, got {other:?}"),
    }

    // Interning path: the concrete signature is not pre-interned.
    let mut fresh = TypeTable::new();
    let fresh_t = fresh.intern(Type::Param(Symbol(60)));
    let fresh_string = fresh.primitive(Primitive::Textus);
    let fresh_err = fresh.primitive(Primitive::Numerus);
    let fresh_generic = fresh.intern(Type::Func(FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: fresh_string,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: fresh_t,
        err: Some(fresh_err),
        is_async: false,
        is_generator: false,
    }));
    let mut fresh_bindings = MirGenericBindings::default();
    fresh_bindings.insert(Symbol(60), MirType::semantic(fresh_string));

    let interned = instantiate_generic_type_with_shapes(
        &mut fresh,
        MirType::semantic(fresh_generic),
        &fresh_bindings,
        &MirShapeBindings::default(),
        Span::default(),
    )
    .expect("generic failable function interning instantiation must succeed");
    match fresh.get(interned.semantic_id()) {
        Type::Func(sig) => assert_eq!(sig.err, Some(fresh_err)),
        other => panic!("expected Func, got {other:?}"),
    }
}

#[test]
fn bounded_ascii_instantiation_finds_interned() {
    // Same latent gap as PromissumFailable: BoundedAscii was missing from
    // type_matches_candidate, so Convert-to-ascii<N> failed MIR instantiate
    // with "missing semantic metadata" even though ascii<N> was interned.
    let mut types = TypeTable::new();
    let n2 = types.intern_index(radix_types::IndexExpr::Literal(2));
    let interned = types.bounded_ascii(n2);

    let resolved = instantiate_existing_generic_type(
        &types,
        MirType::semantic(interned),
        &MirGenericBindings::default(),
        Span::default(),
    )
    .expect("BoundedAscii instantiation must find the interned type");
    assert_eq!(resolved.semantic_id(), interned);
}

#[test]
fn queue_stack_instantiation_finds_interned() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let param = types.intern(Type::Param(Symbol(62)));
    let mut bindings = MirGenericBindings::default();
    bindings.insert(Symbol(62), MirType::semantic(numerus));

    for interned in [types.queue(numerus), types.stack(numerus)] {
        let generic = match types.get(interned) {
            Type::Queue(_) => types.queue(param),
            Type::Stack(_) => types.stack(param),
            other => panic!("expected queue or stack, got {other:?}"),
        };
        let resolved = instantiate_existing_generic_type(
            &types,
            MirType::semantic(generic),
            &bindings,
            Span::default(),
        )
        .expect("queue/stack instantiation must find the interned type");
        assert_eq!(resolved.semantic_id(), interned);
    }

    let capacity = types.intern_index(radix_types::IndexExpr::Literal(3));
    for interned in [
        types.bounded_queue(numerus, capacity),
        types.bounded_stack(numerus, capacity),
    ] {
        let generic = match types.get(interned) {
            Type::BoundedQueue { capacity, .. } => types.bounded_queue(param, *capacity),
            Type::BoundedStack { capacity, .. } => types.bounded_stack(param, *capacity),
            other => panic!("expected bounded queue or stack, got {other:?}"),
        };
        let resolved = instantiate_existing_generic_type(
            &types,
            MirType::semantic(generic),
            &bindings,
            Span::default(),
        )
        .expect("bounded queue/stack instantiation must find the interned type");
        assert_eq!(resolved.semantic_id(), interned);
    }
}

#[test]
fn matrix_instantiation_finds_interned() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let shape = types.rank0_shape();
    let interned = types.intern(Type::Matrix(numerus, shape));

    let param = types.intern(Type::Param(Symbol(60)));
    let generic = types.intern(Type::Matrix(param, shape));

    let mut bindings = MirGenericBindings::default();
    bindings.insert(Symbol(60), MirType::semantic(numerus));

    let resolved = instantiate_existing_generic_type(
        &types,
        MirType::semantic(generic),
        &bindings,
        Span::default(),
    )
    .expect("generic Matrix instantiation must resolve against the interned type");
    assert_eq!(resolved.semantic_id(), interned);
}

#[test]
fn atomic_instantiation_finds_interned() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let interned = types.intern(Type::Atomic(numerus));

    let param = types.intern(Type::Param(Symbol(61)));
    let generic = types.intern(Type::Atomic(param));

    let mut bindings = MirGenericBindings::default();
    bindings.insert(Symbol(61), MirType::semantic(numerus));

    let resolved = instantiate_existing_generic_type(
        &types,
        MirType::semantic(generic),
        &bindings,
        Span::default(),
    )
    .expect("generic Atomic instantiation must resolve against the interned type");
    assert_eq!(resolved.semantic_id(), interned);
}

// ---------------------------------------------------------------------------
// 4A — shape-param binding and substitution
// ---------------------------------------------------------------------------

fn shape_2x8(types: &mut TypeTable) -> IndexId {
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let lit8 = types.intern_index(IndexExpr::Literal(8));
    types.intern_index(IndexExpr::Tuple(vec![lit2, lit8]))
}

fn shape_bd(types: &mut TypeTable) -> IndexId {
    let b = types.intern_index(IndexExpr::Param(Symbol(200)));
    let d = types.intern_index(IndexExpr::Param(Symbol(201)));
    types.intern_index(IndexExpr::Tuple(vec![b, d]))
}

fn f32_ty(types: &mut TypeTable) -> TypeId {
    types.sized_numeric(Primitive::Fractus, radix_types::NumericWidth::F32)
}

#[test]
fn witness_binds_shape_params_from_tensor_shapes() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let expected_shape = shape_bd(&mut types);
    let expected = types.intern(Type::Tensor(element, expected_shape));
    let actual_shape = shape_2x8(&mut types);
    let actual = types.intern(Type::Tensor(element, actual_shape));

    let mut bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    assert!(bind_generic_type_witness_with_shapes(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
        &mut shape_bindings,
    ));
    assert_eq!(shape_bindings.len(), 2);
    let b = shape_bindings.get(&Symbol(200)).copied().expect("B bound");
    let d = shape_bindings.get(&Symbol(201)).copied().expect("D bound");
    assert_eq!(types.get_index(b), &IndexExpr::Literal(2));
    assert_eq!(types.get_index(d), &IndexExpr::Literal(8));
}

#[test]
fn witness_binds_type_and_capacity_params_from_bounded_arrays() {
    let mut types = TypeTable::new();
    let element_param = types.intern(Type::Param(Symbol(300)));
    let capacity_param = types.intern_index(IndexExpr::Param(Symbol(301)));
    let expected = types.bounded_array(element_param, capacity_param);
    let element = f32_ty(&mut types);
    let literal_three = types.intern_index(IndexExpr::Literal(3));
    let actual = types.bounded_array(element, literal_three);

    let mut bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    assert!(bind_generic_type_witness_with_shapes(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
        &mut shape_bindings,
    ));
    let bound = bindings.get(&Symbol(300)).copied().expect("T bound");
    assert!(types.equals(bound.semantic_id(), element));
    let capacity = shape_bindings.get(&Symbol(301)).copied().expect("N bound");
    assert_eq!(types.get_index(capacity), &IndexExpr::Literal(3));
}

#[test]
fn witness_binds_the_element_param_from_a_ranged_list() {
    let mut types = TypeTable::new();
    let element_param = types.intern(Type::Param(Symbol(310)));
    let expected = types.ranged_array(element_param, 1, None);
    let element = f32_ty(&mut types);
    let actual = types.ranged_array(element, 1, None);

    let mut bindings = MirGenericBindings::default();
    assert!(bind_generic_type_witness(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
    ));
    let bound = bindings.get(&Symbol(310)).copied().expect("T bound");
    assert!(types.equals(bound.semantic_id(), element));
}

#[test]
fn witness_refuses_a_ranged_list_of_another_range() {
    let mut types = TypeTable::new();
    let element_param = types.intern(Type::Param(Symbol(311)));
    let expected = types.ranged_array(element_param, 1, None);
    let element = f32_ty(&mut types);
    let actual = types.ranged_array(element, 2, None);

    let mut bindings = MirGenericBindings::default();
    assert!(!bind_generic_type_witness(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
    ));
}

#[test]
fn union_mismatch_undoes_partial_shape_bindings() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let parameter = Symbol(230);
    let param_dim = types.intern_index(IndexExpr::Param(parameter));
    let expected_tensor = types.intern(Type::Tensor(element, param_dim));
    let numerus = types.primitive(Primitive::Numerus);
    let expected_member = types.tuple(vec![expected_tensor, numerus]);
    let expected = types.intern(Type::Union(vec![expected_member]));

    let actual_shape = shape_2x8(&mut types);
    let actual_tensor = types.intern(Type::Tensor(element, actual_shape));
    let textus = types.primitive(Primitive::Textus);
    let actual_member = types.tuple(vec![actual_tensor, textus]);
    let actual = types.intern(Type::Union(vec![actual_member]));

    let mut generic_bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    assert!(!bind_generic_type_witness_with_shapes(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut generic_bindings,
        &mut shape_bindings,
    ));
    // The failed candidate bound the shape param from the tensor before the
    // tuple's second element mismatched; rollback must remove it.
    assert!(shape_bindings.is_empty());
}

#[test]
fn witness_inverts_exact_affine_and_divide_shape_dimensions() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let rows = Symbol(220);
    let columns = Symbol(221);
    let row_param = types.intern_index(IndexExpr::Param(rows));
    let column_param = types.intern_index(IndexExpr::Param(columns));
    let row_minus_one = types.intern_index(IndexExpr::Affine {
        base: row_param,
        offset: -1,
    });
    let column_half = types.intern_index(IndexExpr::Divide {
        numerator: column_param,
        divisor: 2,
    });
    let expected_shape = types.intern_index(IndexExpr::Tuple(vec![row_minus_one, column_half]));
    let three = types.intern_index(IndexExpr::Literal(3));
    let four = types.intern_index(IndexExpr::Literal(4));
    let four_by_three_shape = types.intern_index(IndexExpr::Tuple(vec![three, four]));
    let expected = types.intern(Type::Tensor(element, expected_shape));
    let actual = types.intern(Type::Tensor(element, four_by_three_shape));
    // The MIR binder reuses semantic index identities; it never creates a
    // guessed size absent from the analyzed type table.
    let eight = types.intern_index(IndexExpr::Literal(8));

    let mut bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    assert!(bind_generic_type_witness_with_shapes(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
        &mut shape_bindings,
    ));
    assert_eq!(shape_bindings.get(&rows), Some(&four));
    assert_eq!(shape_bindings.get(&columns), Some(&eight));
}

#[test]
fn witness_inverts_matching_runtime_affine_shape_without_erasing_origin() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let parameter = Symbol(222);
    let generic_base = types.intern_index(IndexExpr::Param(parameter));
    let runtime_base = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(
        DefId(50),
    )));
    let expected_shape = types.intern_index(IndexExpr::Affine {
        base: generic_base,
        offset: -1,
    });
    let actual_shape = types.intern_index(IndexExpr::Affine {
        base: runtime_base,
        offset: -1,
    });
    let expected = types.intern(Type::Tensor(element, expected_shape));
    let actual = types.intern(Type::Tensor(element, actual_shape));

    let mut bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    assert!(bind_generic_type_witness_with_shapes(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
        &mut shape_bindings,
    ));
    assert_eq!(shape_bindings.get(&parameter), Some(&runtime_base));
}

#[test]
fn witness_accepts_identical_nongeneric_affine_and_divide_shapes() {
    let mut types = TypeTable::new();
    let literal = types.intern_index(IndexExpr::Literal(8));
    let affine = types.intern_index(IndexExpr::Affine {
        base: literal,
        offset: -1,
    });
    let divide = types.intern_index(IndexExpr::Divide {
        numerator: literal,
        divisor: 2,
    });
    let mut shape_bindings = MirShapeBindings::default();

    assert!(try_bind_index_witness(
        &types,
        affine,
        affine,
        &mut shape_bindings
    ));
    assert!(try_bind_index_witness(
        &types,
        divide,
        divide,
        &mut shape_bindings
    ));
}

#[test]
fn witness_rejects_contradictory_affine_shape_binding() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let parameter = Symbol(223);
    let generic_base = types.intern_index(IndexExpr::Param(parameter));
    let expected_shape = types.intern_index(IndexExpr::Affine {
        base: generic_base,
        offset: -1,
    });
    let actual_shape = types.intern_index(IndexExpr::Literal(3));
    let four = types.intern_index(IndexExpr::Literal(4));
    let five = types.intern_index(IndexExpr::Literal(5));
    let expected = types.intern(Type::Tensor(element, expected_shape));
    let actual = types.intern(Type::Tensor(element, actual_shape));

    let mut bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    shape_bindings.insert(parameter, five);
    assert!(!bind_generic_type_witness_with_shapes(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
        &mut shape_bindings,
    ));
    assert_eq!(shape_bindings.get(&parameter), Some(&five));
    assert_ne!(shape_bindings.get(&parameter), Some(&four));
}

#[test]
fn witness_rejects_rank_mismatched_tensor_shapes() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    // Expected [B] against actual [2, 8] — rank mismatch must fail closed.
    let b = types.intern_index(IndexExpr::Param(Symbol(210)));
    let expected_shape = types.intern_index(IndexExpr::Tuple(vec![b]));
    let expected = types.intern(Type::Tensor(element, expected_shape));
    let actual_shape = shape_2x8(&mut types);
    let actual = types.intern(Type::Tensor(element, actual_shape));

    let mut bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    assert!(!bind_generic_type_witness_with_shapes(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
        &mut shape_bindings,
    ));
    assert!(shape_bindings.is_empty());
}

#[test]
fn type_contains_param_walks_tensor_shapes() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let shape = shape_bd(&mut types);
    let generic = types.intern(Type::Tensor(element, shape));
    assert!(type_contains_param(&types, generic));

    let shape = shape_2x8(&mut types);
    let concrete = types.intern(Type::Tensor(element, shape));
    assert!(!type_contains_param(&types, concrete));
}

#[test]
fn instantiate_generic_type_with_shapes_substitutes_tensor_shape() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let shape = shape_bd(&mut types);
    let generic = types.intern(Type::Tensor(element, shape));

    let mut shape_bindings = MirShapeBindings::default();
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let lit8 = types.intern_index(IndexExpr::Literal(8));
    shape_bindings.insert(Symbol(200), lit2);
    shape_bindings.insert(Symbol(201), lit8);

    let resolved = instantiate_generic_type_with_shapes(
        &mut types,
        MirType::semantic(generic),
        &MirGenericBindings::default(),
        &shape_bindings,
        Span::default(),
    )
    .expect("shape-param instantiation must resolve");

    let Type::Tensor(resolved_element, resolved_shape) = types.get(resolved.semantic_id()) else {
        panic!("resolved type is not a Tensor");
    };
    assert_eq!(*resolved_element, element);
    let dims = match types.get_index(*resolved_shape) {
        IndexExpr::Tuple(dims) => dims,
        other => panic!("resolved shape is not a tuple: {other:?}"),
    };
    assert_eq!(dims.len(), 2);
    for dim in dims {
        assert!(matches!(types.get_index(*dim), IndexExpr::Literal(_)));
    }
}

#[test]
fn witness_binds_shape_params_through_tuple_elements() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let expected_shape = shape_bd(&mut types);
    let expected_tensor = types.intern(Type::Tensor(element, expected_shape));
    let expected = types.tuple(vec![expected_tensor]);
    let actual_shape = shape_2x8(&mut types);
    let actual_tensor = types.intern(Type::Tensor(element, actual_shape));
    let actual = types.tuple(vec![actual_tensor]);

    let mut bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    assert!(bind_generic_type_witness_with_shapes(
        &types,
        MirType::semantic(expected),
        MirType::semantic(actual),
        &mut bindings,
        &mut shape_bindings,
    ));

    let rows = shape_bindings
        .get(&Symbol(200))
        .copied()
        .expect("row shape parameter bound through tuple");
    let cols = shape_bindings
        .get(&Symbol(201))
        .copied()
        .expect("column shape parameter bound through tuple");
    assert_eq!(types.get_index(rows), &IndexExpr::Literal(2));
    assert_eq!(types.get_index(cols), &IndexExpr::Literal(8));
}

#[test]
fn instantiate_existing_generic_type_recurses_through_tuple_elements() {
    let mut types = TypeTable::new();
    let param = types.intern(Type::Param(Symbol(220)));
    let generic = types.tuple(vec![param]);
    let numerus = types.primitive(Primitive::Numerus);
    let concrete = types.tuple(vec![numerus]);

    let mut bindings = MirGenericBindings::default();
    bindings.insert(Symbol(220), MirType::semantic(numerus));

    let resolved = instantiate_existing_generic_type(
        &types,
        MirType::semantic(generic),
        &bindings,
        Span::default(),
    )
    .expect("type-param instantiation must recurse through tuple elements");
    assert_eq!(resolved.semantic_id(), concrete);
}

#[test]
fn instantiate_generic_type_with_shapes_substitutes_tuple_tensor_shape() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let shape = shape_bd(&mut types);
    let tensor = types.intern(Type::Tensor(element, shape));
    let generic = types.tuple(vec![tensor]);

    let mut shape_bindings = MirShapeBindings::default();
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let lit8 = types.intern_index(IndexExpr::Literal(8));
    shape_bindings.insert(Symbol(200), lit2);
    shape_bindings.insert(Symbol(201), lit8);

    let resolved = instantiate_generic_type_with_shapes(
        &mut types,
        MirType::semantic(generic),
        &MirGenericBindings::default(),
        &shape_bindings,
        Span::default(),
    )
    .expect("shape-param instantiation must recurse through tuple elements");

    assert!(!type_contains_param(&types, resolved.semantic_id()));
    let Type::Tuple(elements) = types.get(resolved.semantic_id()) else {
        panic!("resolved type is not a Tuple");
    };
    let Type::Tensor(_, resolved_shape) = types.get(elements[0]) else {
        panic!("resolved tuple element is not a Tensor");
    };
    assert_eq!(
        i64s_from_literal_index(*resolved_shape, &types),
        Some(vec![2, 8])
    );
}

#[test]
fn unresolved_tuple_tensor_shape_remains_visible_to_survivor_check() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let shape = shape_bd(&mut types);
    let tensor = types.intern(Type::Tensor(element, shape));
    let generic = types.tuple(vec![tensor]);

    let mut shape_bindings = MirShapeBindings::default();
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    shape_bindings.insert(Symbol(200), lit2);

    let resolved = instantiate_generic_type_with_shapes(
        &mut types,
        MirType::semantic(generic),
        &MirGenericBindings::default(),
        &shape_bindings,
        Span::default(),
    )
    .expect("partial shape substitution preserves unresolved params for D6");

    assert!(
        type_contains_param(&types, resolved.semantic_id()),
        "an unresolved tuple child must not escape the survivor check"
    );
}

#[test]
fn type_contains_param_walks_tuple_elements() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let generic_shape = shape_bd(&mut types);
    let generic_tensor = types.intern(Type::Tensor(element, generic_shape));
    let generic = types.tuple(vec![generic_tensor]);
    assert!(type_contains_param(&types, generic));

    let concrete_shape = shape_2x8(&mut types);
    let concrete_tensor = types.intern(Type::Tensor(element, concrete_shape));
    let concrete = types.tuple(vec![concrete_tensor]);
    assert!(!type_contains_param(&types, concrete));
}

#[test]
fn canonical_index_collapses_structurally_equal_shapes() {
    let mut types = TypeTable::new();
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let first = types.intern_index(IndexExpr::Tuple(vec![lit2]));
    let lit2_dup = types.intern_index(IndexExpr::Literal(2));
    let second = types.intern_index(IndexExpr::Tuple(vec![lit2_dup]));
    assert_ne!(first, second);
    assert_eq!(canonical_index(&types, second), first);
}

// ── Phase-4 producer fact: shape-size scalar substitution ────────────────

use crate::{
    MirAggregateKind, MirBlock, MirBlockId, MirCallee, MirConversion, MirConversionFlavor,
    MirFunctionId, MirLocalId, MirPlace, MirPlaceBase, MirProjection, MirRuntimeCall, MirStatement,
    MirStatementKind, MirSwitchCase, MirTempId, MirTerminator, MirTerminatorKind, MirValue,
    MirValueId, MirValueKind,
};

fn shape_test_function(blocks: Vec<MirBlock>) -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks,
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

/// Symbolic rank-2 shape `[rows, cols]` with the two `magnitudo` params.
fn symbolic_shape(types: &mut TypeTable) -> (Symbol, Symbol, IndexId) {
    let rows = Symbol(100);
    let cols = Symbol(101);
    let row_dim = types.intern_index(IndexExpr::Param(rows));
    let col_dim = types.intern_index(IndexExpr::Param(cols));
    let shape = types.intern_index(IndexExpr::Tuple(vec![row_dim, col_dim]));
    (rows, cols, shape)
}

fn shape_size(shape: IndexId, axis: Option<i64>) -> MirOperand {
    MirOperand::Constant(MirConstant::ShapeSize { shape, axis })
}

fn float(value: f64) -> MirOperand {
    MirOperand::Constant(MirConstant::Float(value))
}

/// Collect every constant reachable in a function, in traversal order.
fn collect_constants(function: &mut MirFunction) -> Vec<MirConstant> {
    let mut constants = Vec::new();
    let _ = rewrite_function_constants(function, &mut |constant| {
        constants.push(constant.clone());
        true
    });
    constants
}

fn assert_all_floats(function: &mut MirFunction, expected: &[f64]) {
    let constants = collect_constants(function);
    let mut values = constants
        .iter()
        .map(|constant| match constant {
            MirConstant::Float(value) => *value,
            other => panic!("expected only Float constants after substitution, got {other:?}"),
        })
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    let mut expected = expected.to_vec();
    expected.sort_by(f64::total_cmp);
    assert_eq!(
        values, expected,
        "rewritten constants must be the concrete floats"
    );
}

#[test]
fn substitute_shape_size_rewrites_every_operand_position() {
    let mut types = TypeTable::new();
    let (rows, cols, param_shape) = symbolic_shape(&mut types);
    let lit8 = types.intern_index(IndexExpr::Literal(8));
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let mut shape_bindings = MirShapeBindings::default();
    shape_bindings.insert(rows, lit8);
    shape_bindings.insert(cols, lit2);

    let mut block = MirBlock {
        id: MirBlockId(0),
        statements: Vec::new(),
        terminator: MirTerminator {
            kind: MirTerminatorKind::Unreachable,
            span: Span::default(),
        },
        span: Span::default(),
    };

    // Assign value: a binary whose lhs is a whole-shape shape-size scalar.
    block.statements.push(MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace {
                base: MirPlaceBase::Local(MirLocalId(0)),
                projections: Vec::new(),
            },
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Binary {
                    op: crate::MirBinOp::Add,
                    lhs: shape_size(param_shape, None),
                    rhs: float(1.0),
                },
                ty: MirType::semantic(TypeId(0)),
                span: Span::default(),
            },
        },
        span: Span::default(),
    });

    // Ordinary call args carry per-axis extents.
    block.statements.push(MirStatement {
        kind: MirStatementKind::Call {
            destination: None,
            callee: MirCallee::Function(MirFunctionId(1)),
            args: vec![
                shape_size(param_shape, Some(0)),
                shape_size(param_shape, Some(1)),
            ],
        },
        span: Span::default(),
    });

    // Runtime-call args (the CTO-identified gap: runtime-call args were not
    // rewritten by the old type-only substitution).
    block.statements.push(MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic: crate::MirIntrinsic::Panic,
                args: vec![shape_size(param_shape, Some(-1))],
                return_ty: MirType::semantic(TypeId(0)),
            },
        },
        span: Span::default(),
    });

    // Aggregate field operands.
    block.statements.push(MirStatement {
        kind: MirStatementKind::Construct {
            destination: MirPlace {
                base: MirPlaceBase::Local(MirLocalId(1)),
                projections: Vec::new(),
            },
            aggregate: crate::MirAggregate {
                kind: MirAggregateKind::Tuple,
                ty: MirType::semantic(TypeId(0)),
                fields: crate::MirAggregateFields::Ordered(vec![crate::MirAggregateItem::Operand(
                    shape_size(param_shape, None),
                )]),
            },
        },
        span: Span::default(),
    });

    // Place index projection operand plus a switch-case dispatch value.
    block.terminator = MirTerminator {
        kind: MirTerminatorKind::Switch {
            value: MirOperand::Place(MirPlace {
                base: MirPlaceBase::Local(MirLocalId(2)),
                projections: vec![MirProjection::Index(shape_size(param_shape, None))],
            }),
            cases: vec![MirSwitchCase {
                value: MirConstant::ShapeSize {
                    shape: param_shape,
                    axis: None,
                },
                target: MirBlockId(1),
            }],
            default: MirBlockId(2),
        },
        span: Span::default(),
    };

    let mut function = shape_test_function(vec![block]);
    assert!(
        substitute_shape_size_constants(&mut types, &mut function, &shape_bindings),
        "all bindings are concrete so substitution must fold every term"
    );
    assert!(
        !function_contains_shape_size(&mut function),
        "no shape-size scalar may survive a fully-concrete substitution"
    );
    // 1.0 (rhs) + 16.0 (product) + 8.0 + 2.0 (axes) + 2.0 (negative axis)
    // + 16.0 (aggregate) + 16.0 (index projection) + 16.0 (switch case).
    assert_all_floats(&mut function, &[1.0, 16.0, 8.0, 2.0, 2.0, 16.0, 16.0, 16.0]);
}

#[test]
fn tensor_from_flat_shape_size_uses_finalized_result_shape() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let numerus = types.primitive(Primitive::Numerus);
    let four = types.intern_index(IndexExpr::Literal(4));
    let eight = types.intern_index(IndexExpr::Literal(8));
    let target_shape = types.intern_index(IndexExpr::Tuple(vec![four, eight]));
    let stale_shape = types.intern_index(IndexExpr::Param(Symbol(999)));
    let tensor_ty = MirType::semantic(types.tensor_with_shape(element, target_shape));
    let numerus_ty = MirType::semantic(numerus);

    let mut function = shape_test_function(vec![MirBlock {
        id: MirBlockId(0),
        statements: vec![
            MirStatement {
                kind: MirStatementKind::Construct {
                    destination: MirPlace {
                        base: MirPlaceBase::Temp(MirTempId(0)),
                        projections: Vec::new(),
                    },
                    aggregate: crate::MirAggregate {
                        kind: MirAggregateKind::Array,
                        ty: numerus_ty,
                        fields: crate::MirAggregateFields::Ordered(vec![
                            crate::MirAggregateItem::Operand(MirOperand::Temp(MirTempId(1))),
                            crate::MirAggregateItem::Operand(MirOperand::Temp(MirTempId(2))),
                        ]),
                    },
                },
                span: Span::default(),
            },
            MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace {
                        base: MirPlaceBase::Temp(MirTempId(1)),
                        projections: Vec::new(),
                    }),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Convert(MirConversion {
                            store: None,
                            flavor: MirConversionFlavor::Runtime,
                            source_ty: MirType::semantic(types.primitive(Primitive::Fractus)),
                            target_ty: numerus_ty,
                            params: Vec::new(),
                            recovery: None,
                            struct_defaults: Vec::new(),
                        }),
                        args: vec![shape_size(stale_shape, Some(0))],
                        return_ty: numerus_ty,
                    },
                },
                span: Span::default(),
            },
            MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: Some(MirPlace {
                        base: MirPlaceBase::Temp(MirTempId(2)),
                        projections: Vec::new(),
                    }),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Convert(MirConversion {
                            store: None,
                            flavor: MirConversionFlavor::Runtime,
                            source_ty: MirType::semantic(types.primitive(Primitive::Fractus)),
                            target_ty: numerus_ty,
                            params: Vec::new(),
                            recovery: None,
                            struct_defaults: Vec::new(),
                        }),
                        args: vec![shape_size(stale_shape, Some(1))],
                        return_ty: numerus_ty,
                    },
                },
                span: Span::default(),
            },
            MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
                        args: vec![
                            MirOperand::Constant(MirConstant::Unit),
                            MirOperand::Constant(MirConstant::Unit),
                            MirOperand::Temp(MirTempId(0)),
                        ],
                        return_ty: tensor_ty,
                    },
                },
                span: Span::default(),
            },
        ],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    }]);
    function.return_ty = tensor_ty;

    assert!(
        substitute_tensor_from_flat_shape_size_constants(
            &mut types,
            &mut function,
            &MirShapeBindings::default(),
        ),
        "a finalized TensorFromFlat result supplies the concrete shape"
    );
    assert!(!function_contains_shape_size(&mut function));
    let constants = collect_constants(&mut function);
    assert!(
        constants
            .iter()
            .filter_map(|constant| match constant {
                MirConstant::Float(value) => Some(*value),
                _ => None,
            })
            .eq([4.0, 8.0]),
        "both shape axes must fold from the result shape, got {constants:?}"
    );
}

#[test]
fn fold_shape_size_constant_matches_air_fold_semantics() {
    let mut types = TypeTable::new();
    let (rows, cols, param_shape) = symbolic_shape(&mut types);
    let lit8 = types.intern_index(IndexExpr::Literal(8));
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let mut shape_bindings = MirShapeBindings::default();
    shape_bindings.insert(rows, lit8);
    shape_bindings.insert(cols, lit2);

    let whole = fold_shape_size_constant(&mut types, param_shape, None, &shape_bindings);
    assert_eq!(whole, Some(MirConstant::Float(16.0)));
    let first = fold_shape_size_constant(&mut types, param_shape, Some(0), &shape_bindings);
    assert_eq!(first, Some(MirConstant::Float(8.0)));
    let second = fold_shape_size_constant(&mut types, param_shape, Some(1), &shape_bindings);
    assert_eq!(second, Some(MirConstant::Float(2.0)));
    let last = fold_shape_size_constant(&mut types, param_shape, Some(-1), &shape_bindings);
    assert_eq!(last, Some(MirConstant::Float(2.0)));
    // Out-of-range axes fail closed.
    assert_eq!(
        fold_shape_size_constant(&mut types, param_shape, Some(5), &shape_bindings),
        None
    );
    assert_eq!(
        fold_shape_size_constant(&mut types, param_shape, Some(-3), &shape_bindings),
        None
    );
}

/// sgr C3: a bare `Param` size scalar (the `magnitudo` reference a generic
/// body lowers with `axis: None`) folds to the bound integer — rope dims and
/// other `NumerusIndex` positions are integers. Tuple-shape folds keep the f64
/// AIR-mirror carrier (previous test).
#[test]
fn fold_shape_size_param_substitutes_to_int() {
    let mut types = TypeTable::new();
    let dim = Symbol(102);
    let param_dim = types.intern_index(IndexExpr::Param(dim));
    let bound = types.intern_index(IndexExpr::Literal(64));
    let mut shape_bindings = MirShapeBindings::default();
    shape_bindings.insert(dim, bound);

    assert_eq!(
        fold_shape_size_constant(&mut types, param_dim, None, &shape_bindings),
        Some(MirConstant::Int(64)),
        "the substituted size scalar must be the integer bound, not a float"
    );
}

#[test]
fn substitute_shape_size_fails_closed_on_missing_binding() {
    let mut types = TypeTable::new();
    let (_rows, cols, param_shape) = symbolic_shape(&mut types);
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    // `rows` is missing from the instance bindings.
    let mut shape_bindings = MirShapeBindings::default();
    shape_bindings.insert(cols, lit2);

    let block = MirBlock {
        id: MirBlockId(0),
        statements: vec![MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace {
                    base: MirPlaceBase::Local(MirLocalId(0)),
                    projections: Vec::new(),
                },
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Operand(shape_size(param_shape, None)),
                    ty: MirType::semantic(TypeId(0)),
                    span: Span::default(),
                },
            },
            span: Span::default(),
        }],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    };

    let mut function = shape_test_function(vec![block]);
    assert!(
        !substitute_shape_size_constants(&mut types, &mut function, &shape_bindings),
        "a missing binding must fail closed instead of guessing"
    );
    assert!(
        function_contains_shape_size(&mut function),
        "the unresolved scalar must remain detectable by the survivor net"
    );
}

#[test]
fn substitute_shape_size_fails_closed_on_out_of_range_axis() {
    let mut types = TypeTable::new();
    let (rows, cols, param_shape) = symbolic_shape(&mut types);
    let lit8 = types.intern_index(IndexExpr::Literal(8));
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let mut shape_bindings = MirShapeBindings::default();
    shape_bindings.insert(rows, lit8);
    shape_bindings.insert(cols, lit2);

    let mut function = shape_test_function(vec![MirBlock {
        id: MirBlockId(0),
        statements: vec![MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace {
                    base: MirPlaceBase::Local(MirLocalId(0)),
                    projections: Vec::new(),
                },
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Operand(shape_size(param_shape, Some(7))),
                    ty: MirType::semantic(TypeId(0)),
                    span: Span::default(),
                },
            },
            span: Span::default(),
        }],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    }]);
    assert!(
        !substitute_shape_size_constants(&mut types, &mut function, &shape_bindings),
        "an out-of-range axis must fail closed"
    );
    assert!(function_contains_shape_size(&mut function));
}

#[test]
fn substitute_shape_size_fails_closed_on_infer_and_unspecified_dims() {
    let mut types = TypeTable::new();
    let infer_dim = types.intern_index(IndexExpr::Infer(radix_types::IndexVar(0)));
    let unspecified_dim = types.intern_index(IndexExpr::Unspecified);
    let infer_shape = types.intern_index(IndexExpr::Tuple(vec![infer_dim]));
    let unspecified_shape = types.intern_index(IndexExpr::Tuple(vec![unspecified_dim]));

    let mut function = shape_test_function(vec![MirBlock {
        id: MirBlockId(0),
        statements: vec![MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace {
                    base: MirPlaceBase::Local(MirLocalId(0)),
                    projections: Vec::new(),
                },
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Operand(shape_size(infer_shape, None)),
                    ty: MirType::semantic(TypeId(0)),
                    span: Span::default(),
                },
            },
            span: Span::default(),
        }],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    }]);
    assert!(
        !substitute_shape_size_constants(&mut types, &mut function, &MirShapeBindings::default()),
        "an Infer dim must fail closed"
    );
    assert!(function_contains_shape_size(&mut function));

    function.blocks[0].statements[0] = MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace {
                base: MirPlaceBase::Local(MirLocalId(0)),
                projections: Vec::new(),
            },
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Operand(shape_size(unspecified_shape, None)),
                ty: MirType::semantic(TypeId(0)),
                span: Span::default(),
            },
        },
        span: Span::default(),
    };
    assert!(
        !substitute_shape_size_constants(&mut types, &mut function, &MirShapeBindings::default()),
        "an Unspecified dim must fail closed"
    );
    assert!(function_contains_shape_size(&mut function));
}

#[test]
fn existing_and_mutable_instantiation_walks_are_equivalent() {
    let mut types = TypeTable::new();
    let element = f32_ty(&mut types);
    let generic_shape = shape_bd(&mut types);
    let param = types.intern(Type::Param(Symbol(240)));
    let generic_tensor = types.intern(Type::Tensor(param, generic_shape));
    let generic = types.tuple(vec![generic_tensor]);

    let concrete_shape = shape_2x8(&mut types);
    let concrete_tensor = types.intern(Type::Tensor(element, concrete_shape));
    let concrete = types.tuple(vec![concrete_tensor]);

    let mut generic_bindings = MirGenericBindings::default();
    generic_bindings.insert(Symbol(240), MirType::semantic(element));
    let mut shape_bindings = MirShapeBindings::default();
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let lit8 = types.intern_index(IndexExpr::Literal(8));
    shape_bindings.insert(Symbol(200), lit2);
    shape_bindings.insert(Symbol(201), lit8);

    let existing = instantiate_existing_generic_type_with_shapes(
        &types,
        MirType::semantic(generic),
        &generic_bindings,
        &shape_bindings,
        Span::default(),
    )
    .expect("read-only instantiation must resolve the interned concrete type");
    let mutable = instantiate_generic_type_with_shapes(
        &mut types,
        MirType::semantic(generic),
        &generic_bindings,
        &shape_bindings,
        Span::default(),
    )
    .expect("mutable instantiation must resolve the same concrete type");

    assert_eq!(existing.semantic_id(), concrete);
    assert_eq!(mutable, existing);
}

/// One owned param plus ret, with explicit async/generator modifiers.
fn func_sig(param: TypeId, ret: TypeId, is_async: bool, is_generator: bool) -> FuncSig {
    FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: param,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret,
        err: None,
        is_async,
        is_generator,
    }
}

#[test]
fn function_sig_binder_rejects_async_mismatch() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let expected = func_sig(numerus, numerus, true, false);
    let actual = func_sig(numerus, numerus, false, false);
    let mut generic_bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    let mut seen = FxHashSet::default();

    assert!(!try_bind_function_signature(
        &types,
        &expected,
        &actual,
        &mut generic_bindings,
        &mut shape_bindings,
        &mut seen,
    ));
}

#[test]
fn function_sig_binder_rejects_generator_mismatch() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let expected = func_sig(numerus, numerus, false, true);
    let actual = func_sig(numerus, numerus, false, false);
    let mut generic_bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    let mut seen = FxHashSet::default();

    assert!(!try_bind_function_signature(
        &types,
        &expected,
        &actual,
        &mut generic_bindings,
        &mut shape_bindings,
        &mut seen,
    ));
}

#[test]
fn function_sig_binder_binds_matching_modifiers() {
    // With modifiers equal on both sides, generic binding still proceeds.
    let mut types = TypeTable::new();
    let param = types.intern(Type::Param(Symbol(77)));
    let numerus = types.primitive(Primitive::Numerus);
    let expected = func_sig(numerus, param, true, true);
    let actual = func_sig(numerus, numerus, true, true);
    let mut generic_bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    let mut seen = FxHashSet::default();

    assert!(try_bind_function_signature(
        &types,
        &expected,
        &actual,
        &mut generic_bindings,
        &mut shape_bindings,
        &mut seen,
    ));
    assert_eq!(
        generic_bindings
            .get(&Symbol(77))
            .copied()
            .map(MirType::semantic_id),
        Some(numerus)
    );
}

#[test]
fn ledger_instantiation_finds_interned() {
    // A-107: an already-interned Ledger must resolve through
    // ExistingTypeIndex; without a (Ledger, Ledger) arm in
    // type_matches_candidate the read-only walk reports the concrete type as
    // missing.
    let mut types = TypeTable::new();
    let key = types.primitive(Primitive::Textus);
    let value = types.primitive(Primitive::Numerus);
    let interned = types.intern(Type::Ledger(key, value));

    let value_param = types.intern(Type::Param(Symbol(60)));
    let generic = types.intern(Type::Ledger(key, value_param));
    let mut bindings = MirGenericBindings::default();
    bindings.insert(Symbol(60), MirType::semantic(value));

    let resolved = instantiate_existing_generic_type(
        &types,
        MirType::semantic(generic),
        &bindings,
        Span::default(),
    )
    .expect("generic Ledger instantiation must resolve against the interned type");
    assert_eq!(resolved.semantic_id(), interned);
}

/// `(param) -> numerus` with an explicit `sponte` flag on the one parameter.
fn sponte_func_sig(param: TypeId, ret: TypeId, sponte: bool) -> FuncSig {
    let mut sig = func_sig(param, ret, false, false);
    sig.params[0].sponte = sponte;
    sig
}

#[test]
fn existing_type_index_does_not_match_sponte_twin() {
    // A-108: a candidate signature that differs from an interned one only in
    // `sponte` is a different type (TypeTable::equals compares it); the
    // full-scan fallback must not return the interned twin.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let sponte_twin = types.intern(Type::Func(sponte_func_sig(numerus, numerus, true)));
    let index = ExistingTypeIndex::build(&types);

    let plain = Type::Func(sponte_func_sig(numerus, numerus, false));
    assert_eq!(index.find(&types, &plain), None);
    let exact = Type::Func(sponte_func_sig(numerus, numerus, true));
    assert_eq!(index.find(&types, &exact), Some(sponte_twin));
}

#[test]
fn function_sig_binder_rejects_sponte_mismatch() {
    // A-108: the witness binder compares mode and optional but not sponte,
    // so a sponte slot would bind against a plain one.
    let mut types = TypeTable::new();
    let param = types.intern(Type::Param(Symbol(61)));
    let numerus = types.primitive(Primitive::Numerus);
    let expected = sponte_func_sig(param, numerus, true);
    let actual = sponte_func_sig(numerus, numerus, false);
    let mut generic_bindings = MirGenericBindings::default();
    let mut shape_bindings = MirShapeBindings::default();
    let mut seen = FxHashSet::default();

    assert!(!try_bind_function_signature(
        &types,
        &expected,
        &actual,
        &mut generic_bindings,
        &mut shape_bindings,
        &mut seen,
    ));
}
