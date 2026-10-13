#[test]
fn generic_applied_arg_preserves_index_kind() {
    let mut types = radix_types::TypeTable::new();
    let base = types.intern(radix_types::Type::Param(radix_module::lexer::Symbol(10)));
    let size = types.intern_index(radix_types::IndexExpr::Literal(3));
    let applied = types.intern(radix_types::Type::Applied(
        base,
        vec![radix_types::AppliedArg::Size(size)],
    ));
    assert!(matches!(
        types.get(applied),
        radix_types::Type::Applied(_, args)
            if matches!(args.as_slice(), [radix_types::AppliedArg::Size(id)] if *id == size)
    ));
}

#[test]
fn import_semantic_type_preserves_nested_compound_shape() {
    let mut source = radix_types::TypeTable::new();
    let numerus = source.primitive(radix_types::Primitive::Numerus);
    let array = source.array(numerus);
    let tuple = source.tuple(vec![array, numerus]);
    let input = source.intervallum(tuple);

    let mut target = radix_types::TypeTable::new();
    let mut imported = std::collections::HashMap::new();
    let output =
        super::import_semantic_type_with_nominal(&source, &mut target, input, &mut imported, None);

    let numerus_target = target.primitive(radix_types::Primitive::Numerus);
    let array_target = target.ensure_array(numerus_target);
    let tuple_target = target.tuple(vec![array_target, numerus_target]);
    let intervallum_target = target.intervallum(tuple_target);
    assert_eq!(output, intervallum_target);

    // The second import of the same source id short-circuits through the cache.
    let again =
        super::import_semantic_type_with_nominal(&source, &mut target, input, &mut imported, None);
    assert_eq!(output, again);
}
