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
        .filter(|row| row.ns == Namespace::Exact && row.targets.contains(&Target::HirRust))
        .collect();
    assert_eq!(rows.len(), 17);
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
fn the_store_family_is_the_exact_and_inf_rows_that_apply_a_width_limit() {
    let mut store_rows: Vec<(String, &str)> = TABLE
        .iter()
        .filter(|row| row.kind == RuntimeKind::StoreFn)
        .map(|row| (format!("{:?}", row.ns), row.name))
        .collect();
    store_rows.sort_unstable();
    // `exact` (Rust `store` and `store_inferred`; Go `store`, `wrap`, `clamp_i`,
    // `clamp_u`) and `inf` (Rust `store`; Go the same five).
    assert_eq!(
        store_rows,
        [
            ("Exact".to_owned(), "clamp_i"),
            ("Exact".to_owned(), "clamp_u"),
            ("Exact".to_owned(), "store"),
            ("Exact".to_owned(), "store_inferred"),
            ("Exact".to_owned(), "wrap"),
            ("Inf".to_owned(), "clamp_i"),
            ("Inf".to_owned(), "clamp_u"),
            ("Inf".to_owned(), "store"),
            ("Inf".to_owned(), "store_i"),
            ("Inf".to_owned(), "store_u"),
            ("Inf".to_owned(), "wrap")
        ]
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
        spell_store(Target::HirRust, &EXACT_STORE_INFERRED, &site()),
        Ok("faber::exact::store_inferred".to_owned())
    );
    assert_eq!(
        spell(Target::HirRust, &EXACT_MUL),
        Ok("faber::exact::mul".to_owned())
    );
    assert_eq!(
        spell_store(Target::HirRust, &EXACT_FIT, &site()),
        Err(SpellError::NotStore)
    );
}

#[test]
fn the_go_exact_rows_spell_as_the_flat_rt_names() {
    let go_rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Exact && row.targets.contains(&Target::HirGo))
        .collect();
    // 37 functions and the `ExactX` carrier type.
    assert_eq!(go_rows.len(), 38);
    assert_eq!(
        go_rows
            .iter()
            .filter(|row| row.kind == RuntimeKind::Type)
            .count(),
        1
    );
    assert_eq!(spell_any(Target::HirGo, &EXACT_X), "rt.ExactX");
    for (row, spelled) in [
        (&EXACT_ADD, "rt.ExactAdd"),
        (&EXACT_MUL, "rt.ExactMul"),
        (&EXACT_MOD, "rt.ExactMod"),
        (&EXACT_DIV, "rt.ExactDiv"),
        (&EXACT_POW, "rt.ExactPow"),
        (&EXACT_LT_F, "rt.ExactLtF"),
        (&EXACT_FROM_W, "rt.ExactFromW"),
        (&EXACT_CLAMP_U, "rt.ExactClampU"),
    ] {
        assert_eq!(spell_any(Target::HirGo, row), spelled);
    }
    assert_eq!(
        spell_store(Target::HirGo, &EXACT_STORE, &site()),
        Ok("rt.ExactStore".to_owned())
    );
    // The shared rows keep their Rust spelling.
    assert_eq!(
        spell(Target::HirRust, &EXACT_DIV),
        Ok("faber::exact::div".to_owned())
    );
}

#[test]
fn the_go_dec_rows_spell_as_the_flat_rt_names() {
    let go_rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Dec && row.targets.contains(&Target::HirGo))
        .collect();
    // 17 functions and the `DecCarrier` type; none is store-family (a decimal
    // store has no `HirStoreSite` where an unstored computation is consumed).
    assert_eq!(go_rows.len(), 18);
    assert_eq!(
        go_rows
            .iter()
            .filter(|row| row.kind == RuntimeKind::Type)
            .count(),
        1
    );
    assert!(go_rows.iter().all(|row| row.kind != RuntimeKind::StoreFn));
    assert_eq!(spell_any(Target::HirGo, &DEC_CARRIER), "rt.DecCarrier");
    for (row, spelled) in [
        (&DEC_ADD, "rt.DecAdd"),
        (&DEC_OF, "rt.DecOf"),
        (&DEC_STORE, "rt.DecStore"),
        (&DEC_TRY_STORE, "rt.DecTryStore"),
        (&DEC_TO_INT, "rt.DecToInt"),
        (&DEC_F64, "rt.DecF64"),
        (&DEC_FLOAT_CMP, "rt.DecFloatCmp"),
        (&DEC_TRY_PARSE, "rt.DecTryParse"),
        (&DEC_PARSE, "rt.DecParse"),
    ] {
        assert_eq!(spell_any(Target::HirGo, row), spelled);
    }
}

#[test]
fn the_go_inf_rows_spell_as_the_flat_rt_names() {
    let go_rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Inf && row.targets.contains(&Target::HirGo))
        .collect();
    // 50 functions and the `InfPart` composite-key type.
    assert_eq!(go_rows.len(), 51);
    assert_eq!(
        go_rows
            .iter()
            .filter(|row| row.kind == RuntimeKind::Type)
            .count(),
        1
    );
    // The width limit applies only in the store family.
    let mut store: Vec<&str> = go_rows
        .iter()
        .filter(|row| row.kind == RuntimeKind::StoreFn)
        .map(|row| row.name)
        .collect();
    store.sort_unstable();
    assert_eq!(store, ["clamp_i", "clamp_u", "store_i", "store_u", "wrap"]);
    assert_eq!(spell_any(Target::HirGo, &INF_PART), "rt.InfPart");
    for (row, spelled) in [
        (&INF_Z, "rt.InfZ"),
        (&INF_I, "rt.InfI"),
        (&INF_ADD, "rt.InfAdd"),
        (&INF_LIST_EQ, "rt.InfListEq"),
        (&INF_LT_F, "rt.InfLtF"),
        (&INF_F64, "rt.InfF64"),
        (&INF_FROM_VALOR, "rt.InfFromValor"),
        (&INF_TO_BYTES, "rt.InfToBytes"),
        (&INF_FITS_U, "rt.InfFitsU"),
        (&INF_TO_DEC, "rt.InfToDec"),
        (&INF_FROM_KEY, "rt.InfFromKey"),
    ] {
        assert_eq!(spell_any(Target::HirGo, row), spelled);
    }
    assert_eq!(
        spell_store(Target::HirGo, &INF_STORE_I, &site()),
        Ok("rt.InfStoreI".to_owned())
    );
    assert_eq!(
        spell(Target::HirGo, &INF_STORE_I),
        Err(SpellError::StoreOnly)
    );
}

#[test]
fn the_go_format_rows_spell_as_the_flat_rt_names() {
    let go_rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Format && row.targets.contains(&Target::HirGo))
        .collect();
    // 9 functions and the `FormatSpec` layout-spec type; none is a store row.
    assert_eq!(go_rows.len(), 10);
    assert_eq!(
        go_rows
            .iter()
            .filter(|row| row.kind == RuntimeKind::Type)
            .count(),
        1
    );
    assert!(go_rows.iter().all(|row| row.kind != RuntimeKind::StoreFn));
    for (row, spelled) in [
        (&FORMAT_SPEC, "rt.FormatSpec"),
        (&FORMAT_PAD, "rt.FormatPad"),
        (&FORMAT_SCI, "rt.FormatSci"),
        (&FORMAT_INT, "rt.FormatInt"),
        (&FORMAT_SIGNED, "rt.FormatSigned"),
        (&FORMAT_FLOAT, "rt.FormatFloat"),
        (&FORMAT_TEXT, "rt.FormatText"),
        (&FORMAT_INSTANS, "rt.FormatInstans"),
        (&FORMAT_MAGNUS, "rt.FormatMagnus"),
        (&FORMAT_MAGNUS_SCI, "rt.FormatMagnusSci"),
    ] {
        assert_eq!(spell_any(Target::HirGo, row), spelled);
    }
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
        .filter(|row| row.ns == Namespace::Dec && row.targets.contains(&Target::HirRust))
        .collect();
    assert_eq!(rows.len(), 19);
    for row in rows {
        assert_eq!(row.kind, RuntimeKind::Fn);
        assert!(row.targets.contains(&Target::HirRust));
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
        .filter(|row| row.ns == Namespace::Inf && row.targets.contains(&Target::HirRust))
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
        assert!(row.targets.contains(&Target::HirRust));
        assert_eq!(
            spell_any(Target::HirRust, row),
            format!("faber::inf::{}", row.name)
        );
    }
}

#[test]
fn the_format_rows_spell_as_faber_format_functions_and_the_crate_root_fmt_type() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Format && row.targets.contains(&Target::HirRust))
        .collect();
    assert_eq!(rows.len(), 9);
    for row in rows {
        assert!(row.targets.contains(&Target::HirRust));
        let (kind, spelled) = if row.name == "Fmt" {
            (RuntimeKind::Type, "faber::Fmt".to_owned())
        } else {
            (RuntimeKind::Fn, format!("faber::format::{}", row.name))
        };
        assert_eq!(row.kind, kind);
        assert_eq!(spell_any(Target::HirRust, row), spelled);
    }
}

#[test]
fn the_rust_display_rows_spell_as_faber_display_functions() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Display && row.targets.contains(&Target::HirRust))
        .collect();
    let spelled: Vec<String> = rows
        .iter()
        .map(|row| spell_any(Target::HirRust, row))
        .collect();
    assert_eq!(
        spelled,
        [
            "faber::display::bivalens",
            "faber::display::fractus",
            "faber::display::option",
            "faber::display::option_bivalens",
            "faber::display::option_fractus",
            "faber::display::option_vacuum",
            "faber::display::text_payload",
            "faber::display::valor",
        ]
    );
    assert!(rows.iter().all(|row| row.kind == RuntimeKind::Fn));
}
