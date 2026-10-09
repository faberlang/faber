use super::*;

fn spanish() -> DisplayTokens {
    DisplayTokens {
        true_: "cierto".to_owned(),
        false_: "falso".to_owned(),
        none: "nada".to_owned(),
        tuple: "tupla".to_owned(),
    }
}

#[test]
fn intern_deduplicates_equal_tokens() {
    let mut table = MirDisplayTokens::default();
    assert_eq!(table.intern(&DisplayTokens::latin()), DisplayTokensId(0));
    assert_eq!(table.intern(&spanish()), DisplayTokensId(1));
    assert_eq!(table.intern(&DisplayTokens::latin()), DisplayTokensId(0));
    assert_eq!(table.tokens.len(), 2);
}

#[test]
fn merged_modules_remap_ids_so_each_function_keeps_its_own_entry() {
    let mut entry = MirDisplayTokens::default();
    entry.assign(MirFunctionId(1), &DisplayTokens::latin());
    let mut sibling = MirDisplayTokens::default();
    sibling.assign(MirFunctionId(10), &spanish());
    sibling.assign(MirFunctionId(11), &DisplayTokens::latin());

    entry.absorb(&sibling, |id| MirFunctionId(id.0 + 100));

    assert_eq!(entry.tokens.len(), 2);
    assert_eq!(
        entry.for_function(MirFunctionId(1)),
        Some(&DisplayTokens::latin())
    );
    assert_eq!(entry.for_function(MirFunctionId(110)), Some(&spanish()));
    assert_eq!(
        entry.for_function(MirFunctionId(111)),
        Some(&DisplayTokens::latin())
    );
    assert_eq!(entry.by_function[&MirFunctionId(111)], DisplayTokensId(0));
    assert_eq!(entry.by_function[&MirFunctionId(110)], DisplayTokensId(1));
}
