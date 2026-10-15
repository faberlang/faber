//! Semantic relations over `HirStruct.implements`: contract (`implet`)
//! subsumption at the assignment funnel and its declared negatives. Class
//! inheritance (`extends`/`abstractus`) was removed (DECISIONS D8.1), so a
//! genus relates to other types only through the contracts it declares.
//!
//! E1 (equality-tier-respelling) also pins the scalar equality relation
//! ladder on this configured (en) surface: relaxed `≅`/`≈` admit mixed-width
//! numeric pairs, strict `≡` stays fail-closed — see the law block on
//! `numeric_value_equality_types`.

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

/// Analyze an English-surface source (probe spellings: `class`, `fn`, `type`).
fn analyze_en(source: &str) -> AnalyzedFixture {
    analyze_source_with_config(source, en_config())
}

fn assert_en_accepts(source: &str) {
    let result = analyze_en(source);
    assert!(
        result.is_ok(),
        "expected green, got: {:?}",
        result.diagnostics
    );
}

fn assert_en_rejects_with_issue(source: &str, expected: &str) {
    let result = analyze_en(source);
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

#[test]
fn p5_implements_subsumption_at_arguments_flips_green() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn probe(Greets g) → string {
    return g.greet()
}

main {
    let Hello h ← Hello {}
    probe(h)
}
",
    );
}

#[test]
fn p6_implements_subsumption_at_initializer_flips_green() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn probe() → string {
    let Greets g ← Hello {}
    return g.greet()
}

main {
    probe()
}
",
    );
}

#[test]
fn p8_subsumption_at_returns_flips_green() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn make() → Greets {
    return Hello {}
}

main {
    make()
}
",
    );
}

#[test]
fn p14_end_to_end_virtual_dispatch_checks_green() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Formal implements Greets {
    fn greet() → string { return \"good evening\" }
}

class Casual implements Greets {
    fn greet() → string { return \"yo\" }
}

fn greet_of(Greets g) → string {
    return g.greet()
}

main {
    let Formal a ← Formal {}
    let Casual b ← Casual {}
    let string out1 ← greet_of(a)
    print out1
    let string out2 ← greet_of(b)
    print out2
}
",
    );
}

// NEGATIVES — the declared red set.

#[test]
fn implet_naming_non_implendum_is_rejected() {
    assert_en_rejects_with_issue(
        "class Base {
    mut string message
}

class Weird implements Base {
    mut string note
}

main {
    print \"kind\"
}
",
        "implet_requires_implendum",
    );
}

#[test]
fn invariant_generics_reject_lista_concrete_into_lista_contract() {
    assert_en_rejects_with_issue(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn probe(list<Greets> xs) → void {
    print \"probe\"
}

main {
    let Hello h ← Hello {}
    let list<Hello> hs ← [h]
    probe(hs)
}
",
        "argument_type_mismatch",
    );
}

#[test]
fn genus_does_not_flow_into_an_unrelated_genus() {
    assert_en_rejects_with_issue(
        "class Base {
    mut string message
}

class Other {
    mut string message
}

fn probe(Base b) → string {
    return b.message
}

main {
    let Other o ← Other {message = \"hi\"}
    probe(o)
}
",
        "argument_type_mismatch",
    );
}

// FUNNEL COVERAGE — all four sites plus the `⇥` channel.

#[test]
fn channel_subsumption_resolves_iace_value_type_mismatch() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn failable() → void ⇥ Greets ∪ string {
    let Hello h ← Hello {}
    throw h
}

main {
    do {
        failable()
    } catch err {
        print \"caught\"
    }
}
",
    );
}

#[test]
fn intersection_slot_decomposes_at_the_funnel() {
    assert_en_accepts(
        "interface Readable {
    fn read() → string
}

interface Seekable {
    fn seek() → string
}

type Reversible = Readable ∩ Seekable

class Doc implements Readable ∩ Seekable {
    fn read() → string { return \"r\" }
    fn seek() → string { return \"s\" }
}

fn probe(Reversible r) → void {
    print \"accepted\"
}

main {
    let Doc d ← Doc {}
    probe(d)
}
",
    );
}

#[test]
fn bodyless_genus_method_is_rejected() {
    // Bodyless methods belong to contracts; a genus method needs a body.
    let result = analyze_en(
        "class Concrete {
    fn bodyless() → string
}

main {
    print \"no\"
}
",
    );
    assert!(
        !result.is_ok(),
        "bodyless genus method must not parse, got: {:?}",
        result.diagnostics
    );
}

// EQUALITY RELATIONS — the scalar equality relation ladder on the configured
// surface (E1 contract pin): relaxed ≅/≈ admit the mixed-width carrier pairs
// the widening join accepts; strict ≡ rejects the same pairs fail-closed.

#[test]
fn equality_relation_relaxed_admits_mixed_width_pairs() {
    assert_en_accepts(
        "main {
    assert 1 ∷ i64 ≅ 1 ∷ u64
    assert 1 ∷ i64 ≅ 1.0 ∷ f64
    assert 1 ∷ u64 ≅ 1.0 ∷ f64
    assert 1 ∷ i64 ≈ 1 ∷ u64
}
",
    );
}

#[test]
fn equality_relation_strict_rejects_mixed_width_pairs() {
    assert_en_rejects_with_issue(
        "main {
    assert 1 ∷ i64 ≡ 1 ∷ u64
}
",
        "equality_exact_types_required",
    );
}

// IMPORTED CONTRACTS — a struct exported with its `implet` contracts is
// assignable to the imported contract slot it names, and only to those.

/// The `forma` module's file interface: contract `Figura`, and genus
/// `Circulus` naming it in `implet` iff `circulus_implements`.
fn forma_interface(circulus_implements: bool) -> crate::file_interface::FileInterface {
    use crate::Primitive;
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceLibraryIdentity,
        InterfaceNominalExport, InterfaceNominalKind, InterfaceNominalType, InterfaceStructExport,
        InterfaceStructField, InterfaceTypeSnapshot,
    };
    let identity = InterfaceLibraryIdentity {
        provider: "test".to_owned(),
        package: Some("forma".to_owned()),
        module_path: vec!["forma".to_owned()],
    };
    let figura = InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind: InterfaceNominalKind::Interface,
        name: "Figura".to_owned(),
        identity: Some(identity.clone()),
        kind_guessed: false,
    });
    let mut interface = FileInterface::new().with_identity(identity);
    interface.insert(FileExport {
        name: "Figura".to_owned(),
        kind: FileExportKind::Interface(InterfaceNominalExport {
            name: "Figura".to_owned(),
            params: Vec::new(),
            methods: Vec::new(),
            variants: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
        }),
    });
    interface.insert(FileExport {
        name: "Circulus".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: if circulus_implements {
                vec![figura]
            } else {
                Vec::new()
            },
            name: "Circulus".to_owned(),
            params: Vec::new(),
            fields: vec![InterfaceStructField {
                name: "radius".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                optional: false,
                required: true,
                default: None,
                modifier: Some(radix_syntax::FieldModifier::Varia),
                visibility: radix_syntax::MemberVisibility::Publica,
            }],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    interface
}

fn analyze_forma_consumer(circulus_implements: bool) -> AnalyzedFixture {
    use crate::passes::visibility_annotation::VisibilityTier;
    use std::collections::BTreeMap;
    crate::test_support::analyze_source_with_import_contract(
        "importa ex \"./forma\" forma\n\n\
         functio utere(forma.Figura f) → vacuum {\n}\n\n\
         incipit {\n    varia forma.Circulus c ← forma.Circulus { radius = 1 }\n    utere(c)\n}",
        BTreeMap::from([(
            "forma".to_owned(),
            BTreeMap::from([
                ("Figura".to_owned(), VisibilityTier::Publica),
                ("Circulus".to_owned(), VisibilityTier::Publica),
            ]),
        )]),
        BTreeMap::from([("forma".to_owned(), forma_interface(circulus_implements))]),
    )
}

#[test]
fn imported_struct_is_assignable_to_the_imported_contract_it_implements() {
    let result = analyze_forma_consumer(true);
    assert!(result.is_ok(), "{:?}", result.diagnostics);
}

#[test]
fn imported_struct_that_does_not_name_the_contract_stays_rejected() {
    let result = analyze_forma_consumer(false);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && crate::test_support::issue(err) == Some("argument_type_mismatch")),
        "{:?}",
        result.diagnostics
    );
}
