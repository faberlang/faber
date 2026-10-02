use super::*;
use radix_hir::HirStoreKind;
use std::collections::BTreeSet;

const FIVE: [Target; 5] = [
    Target::HirRust,
    Target::HirTypeScript,
    Target::HirGo,
    Target::HirSwift,
    Target::HirPython,
];
const ALL_FIVE: &[Target] = &FIVE;
const TS_ONLY: &[Target] = &[Target::HirTypeScript];

const fn item(ns: Namespace, name: &'static str, kind: RuntimeKind) -> RuntimeItem {
    RuntimeItem {
        ns,
        name,
        kind,
        targets: ALL_FIVE,
    }
}

fn site() -> HirStoreSite {
    HirStoreSite {
        kind: HirStoreKind::Declaration,
        slot: None,
        inferred_from: None,
        inferred_text: None,
    }
}

fn synthetic() -> Vec<RuntimeItem> {
    vec![
        item(Namespace::Exact, "checked_mul", RuntimeKind::Fn),
        item(Namespace::Exact, "div", RuntimeKind::Fn),
        item(Namespace::Exact, "store_i", RuntimeKind::StoreFn),
        item(Namespace::Exact, "store", RuntimeKind::StoreFn),
        item(Namespace::Dec, "checked_mul", RuntimeKind::Fn),
        item(Namespace::Json, "parse", RuntimeKind::Fn),
        item(Namespace::Json, "Value", RuntimeKind::Type),
        item(Namespace::Result, "Failure", RuntimeKind::Type),
        item(Namespace::Tuple, "get_2", RuntimeKind::Fn),
    ]
}

/// Spell a row through the one door its kind allows.
fn spell_any(target: Target, row: &RuntimeItem) -> String {
    let spelled = if row.kind == RuntimeKind::StoreFn {
        spell_store(target, row, &site())
    } else {
        spell(target, row)
    };
    assert!(spelled.is_ok(), "{target} must spell {row:?}");
    spelled.unwrap_or_default()
}

#[test]
fn one_row_has_five_spellings() {
    let row = item(Namespace::Exact, "checked_mul", RuntimeKind::Fn);
    assert_eq!(
        spell(Target::HirRust, &row),
        Ok("faber::exact::checked_mul".to_owned())
    );
    assert_eq!(
        spell(Target::HirTypeScript, &row),
        Ok("exact.checkedMul".to_owned())
    );
    assert_eq!(
        spell(Target::HirGo, &row),
        Ok("rt.ExactCheckedMul".to_owned())
    );
    assert_eq!(
        spell(Target::HirSwift, &row),
        Ok("Exact.checkedMul".to_owned())
    );
    assert_eq!(
        spell(Target::HirPython, &row),
        Ok("exact.checked_mul".to_owned())
    );
}

#[test]
fn a_type_row_spells_per_target() {
    let row = item(Namespace::Json, "Value", RuntimeKind::Type);
    assert_eq!(spell(Target::HirRust, &row), Ok("faber::Value".to_owned()));
    assert_eq!(
        spell(Target::HirTypeScript, &row),
        Ok("json.Value".to_owned())
    );
    assert_eq!(spell(Target::HirGo, &row), Ok("rt.JsonValue".to_owned()));
    assert_eq!(spell(Target::HirSwift, &row), Ok("Json.Value".to_owned()));
    assert_eq!(spell(Target::HirPython, &row), Ok("json.Value".to_owned()));
}

#[test]
fn a_multiword_type_name_is_unchanged_by_camel_casing() {
    let row = item(Namespace::Result, "TaggedFailure", RuntimeKind::Type);
    assert_eq!(
        spell(Target::HirGo, &row),
        Ok("rt.ResultTaggedFailure".to_owned())
    );
}

#[test]
fn a_target_without_a_runtime_has_no_spelling() {
    let row = item(Namespace::Exact, "div", RuntimeKind::Fn);
    assert_eq!(
        spell(Target::HirHaskell, &row),
        Err(SpellError::NoRuntime(Target::HirHaskell))
    );
}

#[test]
fn spelling_is_injective_over_a_slice_per_target() {
    let rows = synthetic();
    for target in FIVE {
        let spelled: Vec<String> = rows.iter().map(|row| spell_any(target, row)).collect();
        let distinct: BTreeSet<&String> = spelled.iter().collect();
        assert_eq!(
            distinct.len(),
            rows.len(),
            "{target} spellings collide: {spelled:?}"
        );
    }
}

#[test]
fn the_real_table_is_injective_per_target() {
    for target in FIVE {
        let spelled: Vec<String> = TABLE.iter().map(|row| spell_any(target, row)).collect();
        let distinct: BTreeSet<&String> = spelled.iter().collect();
        assert_eq!(distinct.len(), TABLE.len(), "{target} spellings collide");
    }
}

#[test]
fn the_flat_go_rule_collides_when_two_ids_camel_alike() {
    // `a_b` and `aB` both spell `AB`: the injectivity test above would catch
    // such a pair, so it is not vacuous.
    let first = item(Namespace::Exact, "a_b", RuntimeKind::Fn);
    let second = item(Namespace::Exact, "aB", RuntimeKind::Fn);
    assert_eq!(spell(Target::HirGo, &first), spell(Target::HirGo, &second));
    assert_ne!(
        spell(Target::HirRust, &first),
        spell(Target::HirRust, &second)
    );
}

#[test]
fn a_store_row_is_refused_by_spell_and_accepted_by_spell_store() {
    let row = item(Namespace::Exact, "store_i", RuntimeKind::StoreFn);
    for target in FIVE {
        assert_eq!(spell(target, &row), Err(SpellError::StoreOnly), "{target}");
    }
    assert_eq!(
        spell_store(Target::HirGo, &row, &site()),
        Ok("rt.ExactStoreI".to_owned())
    );
    assert_eq!(
        spell_store(Target::HirRust, &row, &site()),
        Ok("faber::exact::store_i".to_owned())
    );
    assert_eq!(
        spell_store(Target::HirTypeScript, &row, &site()),
        Ok("exact.storeI".to_owned())
    );
}

#[test]
fn spell_store_refuses_a_row_that_is_not_a_store_fn() {
    let row = item(Namespace::Exact, "div", RuntimeKind::Fn);
    assert_eq!(
        spell_store(Target::HirGo, &row, &site()),
        Err(SpellError::NotStore)
    );
}

#[test]
fn reserved_names_cover_every_namespace_with_a_row() {
    let rows: Vec<RuntimeItem> = Namespace::ALL
        .iter()
        .map(|ns| item(*ns, "f", RuntimeKind::Fn))
        .collect();
    let mut expected: Vec<String> = Namespace::ALL
        .iter()
        .map(|ns| ns.as_str().to_owned())
        .collect();
    expected.sort();
    assert_eq!(reserved_names(Target::HirTypeScript, &rows), expected);
    assert_eq!(reserved_names(Target::HirPython, &rows), expected);

    let swift = reserved_names(Target::HirSwift, &rows);
    assert_eq!(swift.len(), Namespace::ALL.len());
    for ns in Namespace::ALL {
        assert!(
            swift.contains(&upper_camel(ns.as_str())),
            "swift must reserve the enum for {}",
            ns.as_str()
        );
    }
}

#[test]
fn reserved_names_for_rust_and_go_are_the_package_names() {
    assert_eq!(reserved_names(Target::HirRust, &synthetic()), ["faber"]);
    assert_eq!(reserved_names(Target::HirGo, &synthetic()), ["rt"]);
    assert!(reserved_names(Target::HirHaskell, &synthetic()).is_empty());
}

#[test]
fn reserved_names_skip_namespaces_the_target_does_not_carry() {
    let rows = [
        item(Namespace::Exact, "div", RuntimeKind::Fn),
        RuntimeItem {
            ns: Namespace::Regex,
            name: "find",
            kind: RuntimeKind::Fn,
            targets: TS_ONLY,
        },
    ];
    assert_eq!(
        reserved_names(Target::HirTypeScript, &rows),
        ["exact", "regex"]
    );
    assert_eq!(reserved_names(Target::HirPython, &rows), ["exact"]);
    assert!(reserved_names(Target::HirPython, &[]).is_empty());
}

#[test]
fn namespaces_are_unique_lower_snake_and_alphabetical_in_sets() {
    let spelled: BTreeSet<&str> = Namespace::ALL.iter().map(|ns| ns.as_str()).collect();
    assert_eq!(spelled.len(), Namespace::ALL.len());
    for name in &spelled {
        assert!(name.chars().all(|c| c.is_ascii_lowercase()), "{name}");
    }
}

#[test]
fn runtime_refs_return_a_sorted_set() {
    let refs = RuntimeRefs::new();
    assert!(refs.used().is_empty());
    refs.note(Namespace::Valor);
    refs.note(Namespace::Exact);
    refs.note(Namespace::Valor);
    refs.note(Namespace::Dec);
    assert_eq!(
        refs.used(),
        [Namespace::Dec, Namespace::Exact, Namespace::Valor]
    );
}

#[test]
fn go_imports_is_one_path_or_none() {
    let refs = RuntimeRefs::new();
    assert_eq!(go_imports(&refs), None);
    refs.note(Namespace::Exact);
    refs.note(Namespace::Json);
    assert_eq!(go_imports(&refs), Some("faber/rt"));
}

#[test]
fn the_display_rows_spell_as_the_flat_go_names() {
    let spelled: Vec<String> = [DISPLAY_VERUM, DISPLAY_LIST, DISPLAY_VALOR]
        .iter()
        .map(|row| spell(Target::HirGo, row).unwrap_or_default())
        .collect();
    assert_eq!(
        spelled,
        ["rt.DisplayVerum", "rt.DisplayList", "rt.DisplayValor"]
    );
    assert!(TABLE.contains(&DISPLAY_VERUM));
    assert!(TABLE.contains(&DISPLAY_LIST));
    assert!(TABLE.contains(&DISPLAY_VALOR));
}

#[test]
fn spelling_through_the_refs_records_the_namespace_only_on_success() {
    let refs = RuntimeRefs::new();
    let store = item(Namespace::Exact, "store_i", RuntimeKind::StoreFn);
    assert_eq!(
        refs.spell(Target::HirGo, &store),
        Err(SpellError::StoreOnly)
    );
    assert!(refs.used().is_empty());
    assert_eq!(
        refs.spell(Target::HirGo, &DISPLAY_LIST),
        Ok("rt.DisplayList".to_owned())
    );
    assert_eq!(refs.used(), [Namespace::Display]);
}

#[test]
fn the_exact_rows_spell_as_faber_exact_functions_in_rust() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Exact)
        .collect();
    assert_eq!(rows.len(), 13);
    for row in rows {
        assert_eq!(
            spell_any(Target::HirRust, row),
            format!("faber::exact::{}", row.name)
        );
    }
}

#[test]
fn the_regex_rows_spell_as_faber_regex_functions_in_rust() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Regex && row.targets.contains(&Target::HirRust))
        .collect();
    assert_eq!(rows.len(), 12);
    for row in rows {
        assert_eq!(row.kind, RuntimeKind::Fn);
        assert_eq!(
            spell_any(Target::HirRust, row),
            format!("faber::regex::{}", row.name)
        );
    }
    assert_eq!(
        spell(Target::HirRust, &REGEX_FIND_ALL),
        Ok("faber::regex::find_all".to_owned())
    );
}

#[test]
fn only_the_exact_and_inf_store_rows_are_store_family() {
    let store_rows: Vec<(Namespace, &str)> = TABLE
        .iter()
        .filter(|row| row.kind == RuntimeKind::StoreFn)
        .map(|row| (row.ns, row.name))
        .collect();
    assert_eq!(
        store_rows,
        [(Namespace::Exact, "store"), (Namespace::Inf, "store")]
    );
    assert_eq!(
        spell_store(Target::HirRust, &INF_STORE, &site()),
        Ok("faber::inf::store".to_owned())
    );
    assert_eq!(
        spell(Target::HirRust, &INF_STORE),
        Err(SpellError::StoreOnly)
    );
    assert_eq!(
        spell(Target::HirRust, &EXACT_STORE),
        Err(SpellError::StoreOnly)
    );
    assert_eq!(
        spell_store(Target::HirRust, &EXACT_STORE, &site()),
        Ok("faber::exact::store".to_owned())
    );
    assert_eq!(
        spell_store(Target::HirRust, &EXACT_FIT, &site()),
        Err(SpellError::NotStore)
    );
}

#[test]
fn the_regex_rows_spell_from_the_table_for_typescript() {
    // RX-8: the TypeScript runtime package's `regex` namespace.
    for (row, spelled) in [
        (&REGEX_REGEX, "regex.Regex"),
        (&REGEX_MATCH, "regex.Match"),
        (&REGEX_ERROR, "regex.RegexError"),
        (&REGEX_ESCAPE, "regex.escape"),
    ] {
        assert_eq!(spell(Target::HirTypeScript, row), Ok(spelled.to_owned()));
        assert!(row.targets.contains(&Target::HirTypeScript));
    }
    assert!(reserved_names(Target::HirTypeScript, TABLE).contains(&"regex".to_owned()));
    assert!(!reserved_names(Target::HirGo, TABLE).contains(&"regex".to_owned()));
}

#[test]
fn the_dec_rows_spell_as_faber_dec_functions_in_rust() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Dec)
        .collect();
    assert_eq!(rows.len(), 19);
    for row in rows {
        assert_eq!(row.kind, RuntimeKind::Fn);
        assert_eq!(row.targets, [Target::HirRust]);
        assert_eq!(
            spell_any(Target::HirRust, row),
            format!("faber::dec::{}", row.name)
        );
    }
}

#[test]
fn the_inf_rows_spell_as_faber_inf_functions_in_rust() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Inf)
        .collect();
    assert_eq!(rows.len(), 15);
    for row in rows {
        // `store` is the one store-family row (spelled at a store site).
        let kind = if row.name == "store" {
            RuntimeKind::StoreFn
        } else {
            RuntimeKind::Fn
        };
        assert_eq!(row.kind, kind);
        assert_eq!(row.targets, [Target::HirRust]);
        assert_eq!(
            spell_any(Target::HirRust, row),
            format!("faber::inf::{}", row.name)
        );
    }
}
