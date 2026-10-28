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
fn ts_imports_is_one_sorted_named_import_or_none() {
    let refs = RuntimeRefs::new();
    assert_eq!(ts_imports(&refs), None);
    refs.note(Namespace::Json);
    refs.note(Namespace::Display);
    refs.note(Namespace::Json);
    assert_eq!(
        ts_imports(&refs).as_deref(),
        Some("import { display, json } from \"@faber/runtime\";")
    );
}

#[test]
fn the_ts_display_rows_spell_as_display_namespace_members() {
    let rows = [
        (DISPLAY_VALUE, "display.value"),
        (DISPLAY_NUMERUS, "display.numerus"),
        (DISPLAY_FRACTUS, "display.fractus"),
        (DISPLAY_LIST, "display.list"),
        (DISPLAY_TENSOR, "display.tensor"),
        (DISPLAY_SPARSA, "display.sparsa"),
        (DISPLAY_MAP, "display.map"),
        (DISPLAY_VALOR, "display.valor"),
        (DISPLAY_TAGGED_VALOR, "display.taggedValor"),
    ];
    for (row, expected) in rows {
        assert_eq!(spell(Target::HirTypeScript, &row).as_deref(), Ok(expected));
        assert!(TABLE.contains(&row), "{expected} is a table row");
        assert!(row.targets.contains(&Target::HirTypeScript));
    }
    assert!(reserved_names(Target::HirTypeScript, TABLE).contains(&"display".to_owned()));
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
fn the_exact_rows_spell_as_exact_namespace_members_in_typescript() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Exact && row.targets.contains(&Target::HirTypeScript))
        .collect();
    // The 24 exact-integer helpers of `exact.ts`: 16 shared with Rust or Go and
    // 8 of the TypeScript runtime's own.
    assert_eq!(rows.len(), 24);
    let mut spelled: Vec<String> = rows
        .iter()
        .map(|row| spell_any(Target::HirTypeScript, row))
        .collect();
    spelled.sort_unstable();
    let mut expected: Vec<String> = [
        "abs",
        "add",
        "and",
        "chk",
        "clamp",
        "div",
        "floatRem",
        "hi",
        "join",
        "lo",
        "mod",
        "mul",
        "neg",
        "not",
        "or",
        "pow",
        "shl",
        "shr",
        "store",
        "sub",
        "trueDiv",
        "trueDivF32",
        "wrap",
        "xor",
    ]
    .iter()
    .map(|name| format!("exact.{name}"))
    .collect();
    expected.sort_unstable();
    assert_eq!(spelled, expected);
}

#[test]
fn the_dec_rows_spell_as_dec_namespace_members_in_typescript() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Dec && row.targets.contains(&Target::HirTypeScript))
        .collect();
    // The 18 exported helpers of `dec.ts`: 15 shared with Rust and Go, `fmt_list`
    // shared with Rust, `float_cmp` and `to_int` shared with Go. None is
    // store-family (a decimal store is spelled where an unstored computation is
    // consumed, which has no store site at the call).
    assert_eq!(rows.len(), 18);
    assert!(rows.iter().all(|row| row.kind == RuntimeKind::Fn));
    let mut spelled: Vec<String> = rows
        .iter()
        .map(|row| spell_any(Target::HirTypeScript, row))
        .collect();
    spelled.sort_unstable();
    let mut expected: Vec<String> = [
        "add", "cmp", "div", "f64", "floatCmp", "fmt", "fmtList", "int", "mul", "neg", "of",
        "parse", "rem", "store", "sub", "toInt", "tryParse", "tryStore",
    ]
    .iter()
    .map(|name| format!("dec.{name}"))
    .collect();
    expected.sort_unstable();
    assert_eq!(spelled, expected);
}

#[test]
fn the_inf_rows_spell_as_inf_namespace_members_in_typescript() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Inf && row.targets.contains(&Target::HirTypeScript))
        .collect();
    // The 18 exported helpers of `inf.ts`: 7 shared with Rust and Go (`div`,
    // `shl`, `shr`, `pow`, `approx`, `pack`, `to_dec`), 6 with Go (`mod`, `abs`,
    // `parse`, `from_valor`, `from_bytes`, `to_bytes`), 3 with Rust (`true_div`,
    // `cmp_float`, `store`), and `to_number` and `clamp` of its own. `store` and
    // `clamp` are the store family; every other row is plain.
    assert_eq!(rows.len(), 18);
    for row in &rows {
        let store = matches!(row.name, "store" | "clamp");
        assert_eq!(row.kind == RuntimeKind::StoreFn, store, "{}", row.name);
    }
    let mut spelled: Vec<String> = rows
        .iter()
        .map(|row| spell_any(Target::HirTypeScript, row))
        .collect();
    spelled.sort_unstable();
    let mut expected: Vec<String> = [
        "abs",
        "approx",
        "clamp",
        "cmpFloat",
        "div",
        "fromBytes",
        "fromValor",
        "mod",
        "pack",
        "parse",
        "pow",
        "shl",
        "shr",
        "store",
        "toBytes",
        "toDec",
        "toNumber",
        "trueDiv",
    ]
    .iter()
    .map(|name| format!("inf.{name}"))
    .collect();
    expected.sort_unstable();
    assert_eq!(spelled, expected);
}

#[test]
fn the_format_rows_spell_as_format_namespace_members_in_typescript() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Format && row.targets.contains(&Target::HirTypeScript))
        .collect();
    // The six exported renderers of `format.ts`, all shared with Rust and Go;
    // `magnus` and `magnus_sci` are the Rust/Go `inf` renderers (TypeScript
    // renders a `bigint` through `int`), so they are not here.
    assert_eq!(rows.len(), 6);
    for row in &rows {
        assert_eq!(row.kind, RuntimeKind::Fn, "{}", row.name);
        assert!(row.targets.contains(&Target::HirRust), "{}", row.name);
        assert!(row.targets.contains(&Target::HirGo), "{}", row.name);
    }
    let mut spelled: Vec<String> = rows
        .iter()
        .map(|row| spell_any(Target::HirTypeScript, row))
        .collect();
    spelled.sort_unstable();
    let mut expected: Vec<String> = ["float", "instans", "int", "pad", "sci", "text"]
        .iter()
        .map(|name| format!("format.{name}"))
        .collect();
    expected.sort_unstable();
    assert_eq!(spelled, expected);
}

#[test]
fn the_regex_rows_spell_as_faber_regex_functions_in_rust() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Regex && row.targets.contains(&Target::HirRust))
        .collect();
    assert_eq!(rows.len(), 14);
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
    assert_eq!(
        spell(Target::HirRust, &REGEX_COMPILE_OR_TRAP),
        Ok("faber::regex::compile_or_trap".to_owned())
    );
    assert_eq!(
        spell(Target::HirRust, &REGEX_LITERAL_OR_TRAP),
        Ok("faber::regex::literal_or_trap".to_owned())
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
    // `clamp_u`; TypeScript `store`, `wrap`, `clamp`) and `inf` (Rust `store`;
    // Go `store_i`, `store_u`, `wrap`, `clamp_i`, `clamp_u`; TypeScript `store`,
    // `clamp`).
    assert_eq!(
        store_rows,
        [
            ("Exact".to_owned(), "clamp"),
            ("Exact".to_owned(), "clamp_i"),
            ("Exact".to_owned(), "clamp_u"),
            ("Exact".to_owned(), "store"),
            ("Exact".to_owned(), "store_inferred"),
            ("Exact".to_owned(), "wrap"),
            ("Inf".to_owned(), "clamp"),
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
fn the_go_tensor_vector_and_sparsa_rows_spell_as_the_flat_rt_names() {
    for ns in [Namespace::Tensor, Namespace::Vector, Namespace::Sparsa] {
        let go_rows: Vec<&RuntimeItem> = TABLE
            .iter()
            .filter(|row| row.ns == ns && row.targets.contains(&Target::HirGo))
            .collect();
        // Every shaped carrier has its type row; none is a store row.
        assert_eq!(
            go_rows
                .iter()
                .filter(|row| row.kind == RuntimeKind::Type)
                .count(),
            1,
            "{ns:?}"
        );
        assert!(go_rows.iter().all(|row| row.kind != RuntimeKind::StoreFn));
    }
    for (row, spelled) in [
        (&TENSOR_TENSOR, "rt.TensorTensor"),
        (&TENSOR_ELEMENT_COUNT, "rt.TensorElementCount"),
        (&TENSOR_TO_SPARSA, "rt.TensorToSparsa"),
        (&VECTOR_VECTOR, "rt.VectorVector"),
        (&VECTOR_FROM_LIST, "rt.VectorFromList"),
        (&SPARSA_SPARSA, "rt.SparsaSparsa"),
        (&SPARSA_NEW, "rt.SparsaNew"),
    ] {
        assert_eq!(spell_any(Target::HirGo, row), spelled);
    }
}

#[test]
fn the_go_carrier_rows_spell_as_the_flat_rt_names() {
    for (row, spelled) in [
        (&ORDER_ORDER, "rt.OrderOrder"),
        (&INTERVALLUM_INTERVALLUM, "rt.IntervallumIntervallum"),
        (&INTERVALLUM_VALUE, "rt.IntervallumValue"),
        (&TUPLE_KEY, "rt.TupleKey"),
        (&TUPLE_PARTS, "rt.TupleParts"),
        (&INSTANS_PARSE, "rt.InstansParse"),
        (&VALOR_STRUCT, "rt.ValorStruct"),
    ] {
        assert_eq!(spell_any(Target::HirGo, row), spelled);
        // Go carries these rows only; none is a store row.
        assert_eq!(row.targets, &[Target::HirGo]);
        assert_ne!(row.kind, RuntimeKind::StoreFn);
    }
    // `order.compare` is the shared row: Go spells `rt.OrderCompare` and the
    // TypeScript runtime spells `order.compare` (T1-T13).
    assert_eq!(spell_any(Target::HirGo, &ORDER_COMPARE), "rt.OrderCompare");
    assert_eq!(
        ORDER_COMPARE.targets,
        &[Target::HirGo, Target::HirTypeScript]
    );
    // The interval carrier is the one type among them.
    assert_eq!(INTERVALLUM_INTERVALLUM.kind, RuntimeKind::Type);
}

#[test]
fn the_ts_order_and_dup_rows_spell_as_namespace_members() {
    assert_eq!(
        spell(Target::HirTypeScript, &ORDER_COMPARE).as_deref(),
        Ok("order.compare")
    );
    assert_eq!(
        spell(Target::HirTypeScript, &ORDER_TEXT_COMPARE).as_deref(),
        Ok("order.textCompare")
    );
    assert_eq!(
        spell(Target::HirTypeScript, &DUP_DUPLICATE).as_deref(),
        Ok("dup.duplicate")
    );
    for row in [ORDER_COMPARE, ORDER_TEXT_COMPARE, DUP_DUPLICATE] {
        assert!(TABLE.contains(&row), "{row:?} is a table row");
        assert!(row.targets.contains(&Target::HirTypeScript));
        assert_ne!(row.kind, RuntimeKind::StoreFn);
    }
    let reserved = reserved_names(Target::HirTypeScript, TABLE);
    assert!(reserved.contains(&"order".to_owned()));
    assert!(reserved.contains(&"dup".to_owned()));
    // The text compare is TypeScript-only (Go compares UTF-8 bytes).
    assert_eq!(ORDER_TEXT_COMPARE.targets, &[Target::HirTypeScript]);
    assert_eq!(DUP_DUPLICATE.targets, &[Target::HirTypeScript]);
}

#[test]
fn the_go_sermo_rows_spell_as_the_flat_rt_names() {
    for (row, spelled) in [
        (&SERMO_SERMO, "rt.SermoSermo"),
        (&SERMO_MEUS, "rt.SermoMeus"),
        (&SERMO_TUUS, "rt.SermoTuus"),
        (&SERMO_SCRINIUM, "rt.SermoScrinium"),
        (&SERMO_STATUS, "rt.SermoStatus"),
        (&SERMO_REQUEST, "rt.SermoRequest"),
        (&SERMO_ITEM, "rt.SermoItem"),
        (&SERMO_BYTE, "rt.SermoByte"),
        (&SERMO_BULK, "rt.SermoBulk"),
        (&SERMO_DONE, "rt.SermoDone"),
        (&SERMO_ERROR, "rt.SermoError"),
        (&SERMO_CANCEL, "rt.SermoCancel"),
        (&SERMO_CONVERSATION, "rt.SermoConversation"),
        (&SERMO_CONV_ITEM, "rt.SermoConvItem"),
        (&SERMO_OPEN, "rt.SermoOpen"),
        (&SERMO_SET_OPENER, "rt.SermoSetOpener"),
        (&SERMO_DISPATCH, "rt.SermoDispatch"),
        (&SERMO_INSTALL_ROUTES, "rt.SermoInstallRoutes"),
        (&SERMO_AS_MEUS, "rt.SermoAsMeus"),
        (&SERMO_AS_TUUS, "rt.SermoAsTuus"),
        (&SERMO_READ_TEXT, "rt.SermoReadText"),
        (&SERMO_READ_NUMERUS, "rt.SermoReadNumerus"),
        (&SERMO_DRAIN, "rt.SermoDrain"),
    ] {
        assert_eq!(spell_any(Target::HirGo, row), spelled);
        match row.name {
            "Sermo" | "Meus" | "Tuus" | "Scrinium" | "Status" => {
                assert_eq!(row.targets, &[Target::HirGo, Target::HirTypeScript]);
            }
            _ => assert_eq!(row.targets, &[Target::HirGo]),
        }
        assert_ne!(row.kind, RuntimeKind::StoreFn);
    }
    // The types: the handle, its two views, the frame, the status enum with its
    // seven variants and the conversation record.
    let types = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Sermo && row.kind == RuntimeKind::Type)
        .count();
    assert_eq!(types, 13);
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
    // 10 functions and the `FormatSpec` layout-spec type; none is a store row.
    assert_eq!(go_rows.len(), 11);
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
        (&FORMAT_DECIMAL, "rt.FormatDecimal"),
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
    assert_eq!(rows.len(), 20);
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
    assert_eq!(rows.len(), 10);
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
            "faber::display::bivalens_with",
            "faber::display::DisplayTokens",
            "faber::display::fractus",
            "faber::display::option",
            "faber::display::option_bivalens",
            "faber::display::option_bivalens_with",
            "faber::display::option_fractus",
            "faber::display::option_fractus_with",
            "faber::display::option_vacuum",
            "faber::display::option_vacuum_with",
            "faber::display::option_with",
            "faber::display::text_payload",
            "faber::display::valor",
            "faber::display::valor_with",
        ]
    );
    // `DisplayTokens` is the one type; it stays in `faber::display`.
    assert!(
        rows.iter()
            .all(|row| (row.kind == RuntimeKind::Type) == (row.name == "DisplayTokens"))
    );
}

#[test]
fn the_rust_gen_and_float_rows_spell_as_the_crate_root_types_and_a_float_function() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| {
            matches!(row.ns, Namespace::Gen | Namespace::Float)
                && row.targets.contains(&Target::HirRust)
        })
        .collect();
    let spelled: Vec<(String, RuntimeKind)> = rows
        .iter()
        .map(|row| (spell_any(Target::HirRust, row), row.kind))
        .collect();
    assert_eq!(
        spelled,
        [
            ("faber::Gen".to_owned(), RuntimeKind::Type),
            ("faber::Yield".to_owned(), RuntimeKind::Type),
            ("faber::AsyncCursor".to_owned(), RuntimeKind::Type),
            ("faber::AsyncCursorShared".to_owned(), RuntimeKind::Type),
            ("faber::r#gen::block_on".to_owned(), RuntimeKind::Fn),
            ("faber::r#gen::block_on_tokio".to_owned(), RuntimeKind::Fn),
            ("faber::r#gen::send_or_drop".to_owned(), RuntimeKind::Fn),
            ("faber::float::f16_bits_to_f32".to_owned(), RuntimeKind::Fn),
            ("faber::float::bf16_bits_to_f32".to_owned(), RuntimeKind::Fn),
            ("faber::float::f32_to_f16_bits".to_owned(), RuntimeKind::Fn),
            ("faber::float::f32_to_bf16_bits".to_owned(), RuntimeKind::Fn),
        ]
    );
}

#[test]
fn the_rust_trap_rows_spell_as_faber_trap_functions_and_the_crate_root_trap_type() {
    let rows: Vec<&RuntimeItem> = TABLE
        .iter()
        .filter(|row| row.ns == Namespace::Trap && row.targets.contains(&Target::HirRust))
        .collect();
    let spelled: Vec<(String, RuntimeKind)> = rows
        .iter()
        .map(|row| (spell_any(Target::HirRust, row), row.kind))
        .collect();
    assert_eq!(
        spelled,
        [
            ("faber::Trap".to_owned(), RuntimeKind::Type),
            ("faber::trap::fail".to_owned(), RuntimeKind::Fn),
            ("faber::trap::abort".to_owned(), RuntimeKind::Fn),
            ("faber::trap::exit_on_trap".to_owned(), RuntimeKind::Fn),
            ("faber::trap::raise".to_owned(), RuntimeKind::Fn),
            ("faber::trap::some".to_owned(), RuntimeKind::Fn),
            ("faber::trap::ok".to_owned(), RuntimeKind::Fn),
            ("faber::trap::ok_static".to_owned(), RuntimeKind::Fn),
            ("faber::trap::require".to_owned(), RuntimeKind::Fn),
            ("faber::trap::unreachable".to_owned(), RuntimeKind::Fn),
        ]
    );
    assert!(rows.iter().all(|row| row.targets == [Target::HirRust]));
    assert_eq!(Namespace::Trap.as_str(), "trap");
}

#[test]
fn the_trap_namespace_is_carried_by_rust_alone() {
    let refs = RuntimeRefs::new();
    assert_eq!(
        refs.spell(Target::HirRust, &TRAP_SOME),
        Ok("faber::trap::some".to_owned())
    );
    assert_eq!(refs.used(), [Namespace::Trap]);
    assert!(
        reserved_names(Target::HirTypeScript, TABLE)
            .iter()
            .all(|name| name != "trap")
    );
    assert!(
        reserved_names(Target::HirPython, TABLE)
            .iter()
            .all(|name| name != "trap")
    );
    assert!(
        reserved_names(Target::HirSwift, TABLE)
            .iter()
            .all(|name| name != "Trap")
    );
}

#[test]
fn modulus_rows_cover_every_ring_operation_and_width_for_typescript() {
    let rows: Vec<_> = TABLE
        .iter()
        .filter(|item| item.ns == Namespace::Modulus)
        .collect();
    assert_eq!(rows.len(), 96);
    let refs = RuntimeRefs::default();
    for width in ["u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64"] {
        for operation in [
            "add", "sub", "mul", "and", "or", "xor", "neg", "bit_not", "div", "mod", "shl", "shr",
        ] {
            let name = format!("{operation}_{width}");
            let row = rows
                .iter()
                .find(|row| row.name == name)
                .expect("ring row exists");
            assert_eq!(row.kind, RuntimeKind::Fn);
            assert_eq!(row.targets, &[Target::HirTypeScript]);
            let op = if operation == "bit_not" {
                "bitNot"
            } else {
                operation
            };
            let suffix = width.to_ascii_uppercase();
            assert_eq!(
                refs.spell(Target::HirTypeScript, row),
                Ok(format!("modulus.{op}{suffix}"))
            );
        }
    }
    assert_eq!(refs.used(), vec![Namespace::Modulus]);
    assert_eq!(
        ts_imports(&refs),
        Some("import { modulus } from \"@faber/runtime\";".to_owned())
    );
    assert!(
        reserved_names(Target::HirTypeScript, TABLE)
            .iter()
            .any(|name| name == "modulus")
    );
}

#[test]
fn typescript_json_valor_variant_tuple_atomic_rows_are_registered_and_reserved() {
    for (row, expected) in [
        (&JSON_VALUE, "json.Value"),
        (&JSON_PARSE, "json.parse"),
        (&JSON_STRINGIFY, "json.stringify"),
        (&JSON_PARSE_BIG_INT, "json.parseBigInt"),
        (&JSON_STRINGIFY_BIG_INT, "json.stringifyBigInt"),
        (&VALOR_BOX, "valor.box"),
        (&VALOR_PAYLOAD, "valor.payload"),
        (&VALOR_IS, "valor.is"),
        (&VALOR_IS_INTEGER, "valor.isInteger"),
        (&VARIANT_VALUE, "variant.value"),
        (&TUPLE_MAP, "tuple.TupleMap"),
        (&ATOMIC_ATOMIC, "atomic.Atomic"),
    ] {
        assert!(
            TABLE.contains(row),
            "{expected} is missing from namespace hygiene"
        );
        assert_eq!(spell(Target::HirTypeScript, row), Ok(expected.to_owned()));
        assert!(reserved_names(Target::HirTypeScript, TABLE).contains(&row.ns.as_str().to_owned()));
    }
}

#[test]
fn typescript_conversation_and_registry_carriers_have_runtime_rows() {
    for (row, expected) in [
        (&SERMO_SERMO, "sermo.Sermo"),
        (&SERMO_SCRINIUM, "sermo.Scrinium"),
        (&SERMO_MEUS, "sermo.Meus"),
        (&SERMO_TUUS, "sermo.Tuus"),
        (&SERMO_STATUS, "sermo.Status"),
        (&ROUTES_TABLE, "routes.table"),
        (&ROUTES_HANDLER, "routes.Handler"),
        (&ROUTES_REGISTRY, "routes.Registry"),
    ] {
        assert_eq!(spell_any(Target::HirTypeScript, row), expected);
        assert!(TABLE.contains(row));
    }
}
