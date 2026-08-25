//! U0 (type-algebra-error-lifecycle): `∩` grammar + representation, implements
//! as a conjunctive list, R1/R2 fail-closed posture, and ruling (g) —
//! signature-position holes banned, binding holes stay legal.
//!
//! Sources use the English locale pack so the fixtures read like the corpus
//! (`interface`, `class`, `fn`, `typus`).

use crate::locale::LocalePack;
use crate::test_support::{analyze_source_with_config, AnalyzedFixture};
use crate::PassConfig;
use radix_target::Target;

fn en_config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(LocalePack::from_toml_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml"),
    )
    .expect("load English locale pack"));
    config
}

fn analyze_en(source: &str) -> AnalyzedFixture {
    let result = analyze_source_with_config(source, en_config());
    assert!(
        result.is_ok(),
        "expected green, got: {:?}",
        result.diagnostics
    );
    result
}

fn assert_en_rejects_with_issue(source: &str, expected: &str) {
    let result = analyze_source_with_config(source, en_config());
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && crate::test_support::issue(err) == Some(expected)),
        "expected diagnostic issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

fn assert_en_rejects(source: &str) {
    let result = analyze_source_with_config(source, en_config());
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
}

const INTERFACES: &str = "interface Readable {
    fn read() → string
}
interface Seekable {
    fn seek() → string
}
";

#[test]
fn intersection_alias_round_trips_green() {
    analyze_en(
        "interface Readable {
    fn read() → string
}
interface Seekable {
    fn seek() → string
}
type Reversible = Readable ∩ Seekable

fn probe(Reversible r) → void {
    print r
}
",
    );
}

#[test]
fn intersection_implements_flattens_like_the_comma_list() {
    let cap_form = analyze_en(&format!(
        "{INTERFACES}class Doc implements Readable ∩ Seekable {{
    fn read() → string {{ return \"r\" }}
    fn seek() → string {{ return \"s\" }}
}}
"
    ));
    let comma_form = analyze_en(&format!(
        "{INTERFACES}class Doc implements Readable, Seekable {{
    fn read() → string {{ return \"r\" }}
    fn seek() → string {{ return \"s\" }}
}}
"
    ));

    let implements_names = |unit: &AnalyzedFixture| -> Vec<String> {
        let hir = unit.hir.as_ref().expect("hir");
        let target = hir
            .items
            .iter()
            .find_map(|item| match &item.kind {
                radix_hir::HirItemKind::Struct(struct_item)
                    if unit.interner.resolve(struct_item.name) == "Doc" =>
                {
                    Some(struct_item)
                }
                _ => None,
            })
            .expect("struct item");
        target
            .implements
            .iter()
            .map(|def_id| {
                hir.items
                    .iter()
                    .find_map(|item| match &item.kind {
                        radix_hir::HirItemKind::Interface(interface) if item.def_id == *def_id => {
                            Some(unit.interner.resolve(interface.name).to_owned())
                        }
                        _ => None,
                    })
                    .expect("implemented interface item")
            })
            .collect()
    };
    assert_eq!(implements_names(&cap_form), implements_names(&comma_form));
    assert_eq!(
        implements_names(&cap_form),
        vec!["Readable".to_owned(), "Seekable".to_owned()]
    );
}

#[test]
fn r1_implements_union_is_rejected() {
    assert_en_rejects(&format!(
        "{INTERFACES}class Doc implements Readable ∪ Seekable {{
    fn read() → string {{ return \"r\" }}
    fn seek() → string {{ return \"s\" }}
}}
"
    ));
}

#[test]
fn r1_implements_hole_is_rejected() {
    assert_en_rejects(
        "interface Readable {
    fn read() → string
}
class Doc implements _ {
    fn read() → string { return \"r\" }
}
",
    );
}

#[test]
fn r2_primitive_intersection_is_rejected_in_alias() {
    assert_en_rejects_with_issue("type Bad = numerus ∩ fractus", "intersection_primitive_operand");
}

#[test]
fn r2_primitive_intersection_is_rejected_in_annotation() {
    assert_en_rejects_with_issue(
        "fn probe(numerus ∩ fractus both) → numerus {
    return 0
}
",
        "intersection_primitive_operand",
    );
}

#[test]
fn g_return_hole_is_rejected() {
    assert_en_rejects_with_issue(
        "fn probe() → _ {
    return 5
}
",
        "signature_position_hole",
    );
}

#[test]
fn g_error_channel_hole_is_rejected() {
    assert_en_rejects_with_issue(
        "fn probe() → string ⇥ _ {
    return \"x\"
}
",
        "signature_position_hole",
    );
}

#[test]
fn g_binding_holes_stay_legal() {
    analyze_en(
        "main {
    const _ v ← 5
    print v
}
",
    );
    analyze_en(
        "fn pair() → numerus ∪ string {
    return 5
}
main {
    const ∪ u ← pair()
    print u
}
",
    );
}

#[test]
fn channel_union_stays_green() {
    analyze_en(
        "fn load(string path) → string ⇥ numerus ∪ string {
    return \"data\"
}
",
    );
}

#[test]
fn intersection_binds_tighter_than_union() {
    // `(Readable ∩ Seekable) ∪ Doc` — the union's first member is the
    // intersection; every member is nominal so it checks green.
    analyze_en(&format!(
        "{INTERFACES}class Doc {{ }}

type Mixed = Readable ∩ Seekable ∪ Doc
"
    ));
}

#[test]
fn intersection_alias_transparency() {
    // An alias and its unfolded intersection are the same type: both spellings
    // of the parameter slot lower to one TypeId.
    let unit = analyze_en(&format!(
        "{INTERFACES}type Reversible = Readable ∩ Seekable

fn via_alias(Reversible r) → void {{
    print r
}}
fn via_members(Readable ∩ Seekable r) → void {{
    print r
}}
"
    ));
    let hir = unit.hir.as_ref().expect("hir");
    let param_ty = |fn_name: &str| -> radix_types::TypeId {
        hir.items
            .iter()
            .find_map(|item| match &item.kind {
                radix_hir::HirItemKind::Function(func)
                    if unit.interner.resolve(func.name) == fn_name =>
                {
                    Some(func.params[0].ty)
                }
                _ => None,
            })
            .expect("function item")
    };
    // Alias transparency: the alias TypeId and the unfolded intersection are
    // distinct arena entries but semantically equal (`equals` peels Alias).
    assert!(unit
        .types
        .equals(param_ty("via_alias"), param_ty("via_members")));
}
