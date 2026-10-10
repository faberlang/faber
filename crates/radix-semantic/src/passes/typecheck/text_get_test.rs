//! need: the text `get` read (la `accipe`) is the optional read: a
//! one-character `textus ∪ nihil`, `none` past the end or at a negative index.
//! It is the same shape as `lista.get`; only `at` (`apud`) is failable.

use crate::Type;
use crate::test_support::{analyze_ok, assert_accepts, entry_local_tys};

#[test]
fn text_get_infers_optional_textus() {
    let unit = analyze_ok(
        r#"incipit {
    fixum textus s ← "hé"
    fixum _ found ← s.accipe(0)
    nota found
}"#,
    );
    let tys = entry_local_tys(&unit);
    assert_eq!(tys.len(), 2, "one textus local and one found local");
    assert!(
        matches!(unit.types.get(tys[1]), Type::Option(inner)
            if matches!(unit.types.get(*inner), Type::Primitive(crate::Primitive::Textus))),
        "text get must return textus ∪ nihil, got {:?}",
        unit.types.get(tys[1])
    );
}

#[test]
fn text_get_composes_with_vel() {
    assert_accepts(
        r#"incipit {
    fixum textus s ← "hé"
    fixum textus picked ← s.accipe(9) vel "?"
    nota picked
}"#,
    );
}
