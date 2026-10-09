use super::*;

#[test]
fn type_table_seeds_primitives() {
    let table = TypeTable::new();
    for prim in [
        Primitive::Textus,
        Primitive::Numerus,
        Primitive::Fractus,
        Primitive::Bivalens,
        Primitive::Nihil,
        Primitive::Vacuum,
        Primitive::Numquam,
        Primitive::Ignotum,
        Primitive::Octeti,
        Primitive::Regex,
        Primitive::Match,
        Primitive::Json,
        Primitive::Valor,
        Primitive::Ascii,
        Primitive::Instans,
    ] {
        let id = table.primitive(prim);
        assert!(
            matches!(table.get(id), Type::Primitive(p) if *p == prim),
            "expected {prim:?} to be seeded, got {id:?}"
        );
    }
}
