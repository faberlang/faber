//! U0 (type-algebra-error-lifecycle): `∩` grammar + representation, implements
//! as a conjunctive list, R1/R2 fail-closed posture, and ruling (g) —
//! signature-position holes banned, binding holes stay legal.
//!
//! Sources use the English locale pack so the fixtures read like the corpus
//! (`interface`, `class`, `fn`, `typus`).

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config};
use radix_target::Target;

fn en_config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
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
                        radix_hir::HirItemKind::Interface(interface)
                            if item.def_id == def_id.contract =>
                        {
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
    assert_en_rejects_with_issue("type Bad = int ∩ float", "intersection_primitive_operand");
}

#[test]
fn r2_primitive_intersection_is_rejected_in_annotation() {
    assert_en_rejects_with_issue(
        "fn probe(int ∩ float both) → int {
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
        "fn pair() → int ∪ string {
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
        "fn load(string path) → string ⇥ int ∪ string {
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
    assert!(
        unit.types
            .equals(param_ty("via_alias"), param_ty("via_members"))
    );
}

// ---------------------------------------------------------------------------
// Qualified bare generic names (`lib.Res`) follow the unqualified rule
// ---------------------------------------------------------------------------

/// Producer-shaped interface: `lib` exports the generic union `Res<T>`.
fn qualified_generic_union_interface() -> crate::file_interface::FileInterface {
    use crate::file_interface::{
        FileExport, FileExportKind, InterfaceEnumVariant, InterfaceGenericParam,
        InterfaceNominalExport, InterfaceStructField, InterfaceTypeSnapshot,
    };

    let mut interface = crate::file_interface::FileInterface::new();
    interface.insert(FileExport {
        name: "Res".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            params: vec![InterfaceGenericParam {
                name: "T".to_owned(),
                kind: radix_syntax::GenericParamKind::Typus,
            }],
            name: "Res".to_owned(),
            methods: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
            variants: vec![InterfaceEnumVariant {
                name: "Plena".to_owned(),
                fields: vec![InterfaceStructField {
                    name: "res".to_owned(),
                    ty: InterfaceTypeSnapshot::Primitive(crate::Primitive::Numerus),
                    optional: false,
                    required: true,
                    default: None,
                    modifier: None,
                    visibility: radix_syntax::MemberVisibility::Publica,
                }],
            }],
        }),
    });
    interface
}

fn analyze_lib_consumer(source: &str) -> AnalyzedFixture {
    crate::test_support::analyze_source_with_import_contract(
        source,
        std::collections::BTreeMap::from([(
            "lib".to_owned(),
            std::collections::BTreeMap::from([(
                "Res".to_owned(),
                crate::passes::visibility_annotation::VisibilityTier::Publica,
            )]),
        )]),
        std::collections::BTreeMap::from([("lib".to_owned(), qualified_generic_union_interface())]),
    )
}

fn has_arity_mismatch(unit: &AnalyzedFixture) -> bool {
    unit.diagnostics.iter().any(|err| {
        err.is_error() && crate::test_support::issue(err) == Some("generic_argument_arity_mismatch")
    })
}

#[test]
fn qualified_bare_generic_in_parameter_position_is_an_arity_mismatch() {
    // The unqualified `Res r` is already rejected; `lib.Res r` must be too.
    let unit = analyze_lib_consumer(
        r#"importa ex "./lib" lib

functio probe(lib.Res r) → numerus {
    redde 1
}"#,
    );
    assert!(
        has_arity_mismatch(&unit),
        "a bare qualified generic outside construction must be an arity mismatch: {:?}",
        unit.diagnostics
    );
}

#[test]
fn qualified_applied_generic_in_parameter_position_is_accepted() {
    let unit = analyze_lib_consumer(
        r#"importa ex "./lib" lib

functio probe(lib.Res<numerus> r) → numerus {
    redde 1
}"#,
    );
    assert!(
        !has_arity_mismatch(&unit),
        "an applied qualified generic must not report an arity mismatch: {:?}",
        unit.diagnostics
    );
}

#[test]
fn qualified_bare_generic_ascription_in_construction_is_admitted() {
    // Construction contexts (`allow_bare_generic_construction`) infer the
    // witnesses, so the bare qualified name is not an arity mismatch there.
    let unit = analyze_lib_consumer(
        r#"importa ex "./lib" lib

functio probe() → numerus {
    fixum x ← finge lib.Res.Plena { res = 3 } ∷ lib.Res
    redde 1
}"#,
    );
    assert!(
        !has_arity_mismatch(&unit),
        "a bare qualified generic in a construction ascription must be admitted: {:?}",
        unit.diagnostics
    );
}
