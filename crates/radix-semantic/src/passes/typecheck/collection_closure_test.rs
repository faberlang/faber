//! F1-SET-FILTER: set predicate closure typecheck pins.
//!
//! `copia<T>.filtrata(pred)` / en `set<T>.filter(pred)` mirrors the live
//! Lista predicate path: one-argument predicate over the element type,
//! result is `set<T>` (set uniqueness preserved — the result stays a copia,
//! never a lista). No new admission machinery: these rows ride the existing
//! `Predicate` param kind and `SetInner` return kind.

use crate::locale::LocalePack;
use radix_target::Target;

fn en_pack() -> LocalePack {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../stdlib/locale/en/pack.toml");
    LocalePack::from_toml_path(&path).expect("installed en pack should load")
}

#[test]
fn set_filtrata_named_predicate_returns_set_of_element() {
    crate::test_support::assert_accepts(
        r"functio keep(numerus n) → bivalens {
    redde n > 2
}

functio large(copia<numerus> s) → copia<numerus> {
    redde s.filtrata(keep)
}",
    );
}

#[test]
fn set_filtrata_inline_predicate_closure_typechecks() {
    crate::test_support::assert_accepts(
        r"functio large(copia<numerus> s) → copia<numerus> {
    redde s.filtrata(_ n ∴ n > 2)
}",
    );
}

#[test]
fn set_filtrata_result_is_a_set_not_a_list() {
    let unit = crate::test_support::analyze_ok(
        r"functio large(copia<numerus> s) → copia<numerus> {
    redde s.filtrata(_ n ∴ n > 2)
}",
    );
    let hir = unit.hir.as_ref().expect("HIR");
    let item = hir.items.first().expect("function item");
    let radix_hir::HirItemKind::Function(function) = &item.kind else {
        panic!("expected function item, got {:?}", item.kind);
    };
    let Some(ret) = function.ret_ty else {
        panic!("return type must be inferred");
    };
    assert!(
        matches!(
            unit.types.get(ret),
            crate::Type::Set(element)
                if unit.types.equals(*element, unit.types.primitive(crate::Primitive::Numerus))
        ),
        "filtrata on a copia receiver must return set<T>"
    );
}

#[test]
fn set_filtrata_result_is_not_a_lista() {
    crate::test_support::assert_rejects_with_issue(
        r"functio bad(copia<numerus> s) → lista<numerus> {
    redde s.filtrata(_ n ∴ n > 2)
}",
        "return_type_mismatch",
    );
}

#[test]
fn set_filtrata_rejects_wrong_element_predicate() {
    crate::test_support::assert_rejects_with_issue(
        r#"functio keep(textus t) → bivalens {
    redde t.longitudo() > 0
}

functio bad(copia<numerus> s) → copia<numerus> {
    redde s.filtrata(keep)
}"#,
        "argument_type_mismatch",
    );
}

#[test]
fn en_set_filter_surface_resolves_to_filtrata() {
    let source = r"fn keep(int n) → bool {
    return n > 2
}

fn large(set<int> s) → set<int> {
    return s.filter(keep)
}";
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(en_pack());
    let result = crate::test_support::analyze_source_with_config(source, config);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn en_set_filter_result_is_not_a_list() {
    let source = r"fn keep(int n) → bool {
    return n > 2
}

fn bad(set<int> s) → list<int> {
    return s.filter(keep)
}";
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(en_pack());
    let result = crate::test_support::analyze_source_with_config(source, config);
    assert!(!result.is_ok(), "set filter result must be a set, not a list");
}
