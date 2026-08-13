use super::*;
use radix_types::{DefId, Primitive};

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
