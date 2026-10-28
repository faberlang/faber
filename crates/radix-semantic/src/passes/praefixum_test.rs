//! Coverage for `praefixum { … }` static initializers (DECISIONS D8.4–D8.6).
//!
//! Contract:
//! - `praefixum` is legal only as a `generis` initializer; elsewhere it is
//!   SEM064 `praefixum_outside_constant`;
//! - its body type-checks like a function body returning the declared type;
//! - a body that reaches `ad`, await, async, `externa`, input, or output —
//!   directly or through Faber callees — is SEM064 `praefixum_forbidden`,
//!   naming the effect and the call chain;
//! - a static that reads itself through a `praefixum` body is SEM007
//!   `constant_cycle`;
//! - evaluation order puts every static a body reads before it.

use super::praefixum::evaluation_order;
use crate::SemanticError;
use crate::test_support::{analyze_source_la, assert_rejects_with_issue_la, issue};

fn arg<'a>(err: &'a SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

/// The single error diagnostic carrying `expected`.
fn rejection(source: &str, expected: &str) -> SemanticError {
    let result = analyze_source_la(source);
    result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && issue(err) == Some(expected))
        .cloned()
        .unwrap_or_else(|| panic!("expected {expected:?}, got: {:?}", result.diagnostics))
}

#[test]
fn loop_body_with_callee_is_accepted() {
    let result = analyze_source_la(
        "functio quadratum(numerus n) → numerus {\n    redde n * n\n}\n\
         fixum numerus SUMMA = praefixum {\n    varia numerus s ← 0\n    \
         itera ab 1‥4 sit i {\n        s ← s + quadratum(i)\n    }\n    redde s\n}\n",
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn result_must_type_check_as_the_declared_type() {
    assert_rejects_with_issue_la(
        "fixum numerus N = praefixum {\n    redde \"non numerus\"\n}\n",
        "return_type_mismatch",
    );
}

#[test]
fn praefixum_outside_a_static_is_rejected() {
    assert_rejects_with_issue_la(
        "incipit {\n    sit numerus n ← praefixum {\n        redde 1\n    }\n}\n",
        "praefixum_outside_constant",
    );
}

#[test]
fn direct_ad_is_forbidden() {
    let err = rejection(
        "fixum textus T = praefixum {\n    \
         sit textus t ← ad 'runtime:echo' (\"salve\") ↦ textus\n    redde t\n}\n",
        "praefixum_forbidden",
    );
    assert_eq!(arg(&err, "effect"), Some("ad"));
    assert_eq!(arg(&err, "chain"), Some("T"));
}

#[test]
fn transitive_ad_names_the_call_chain() {
    let err = rejection(
        "functio ima() → textus {\n    \
         sit textus t ← ad 'runtime:echo' (\"salve\") ↦ textus\n    redde t\n}\n\
         functio media() → textus {\n    redde ima()\n}\n\
         fixum textus T = praefixum {\n    redde media()\n}\n",
        "praefixum_forbidden",
    );
    assert_eq!(arg(&err, "effect"), Some("ad"));
    assert_eq!(arg(&err, "chain"), Some("T → media → ima"));
}

#[test]
fn async_callee_is_forbidden() {
    let err = rejection(
        "functio lente() fiet → numerus {\n    redde 1\n}\n\
         fixum numerus N = praefixum {\n    sit _ f ← lente()\n    redde 1\n}\n",
        "praefixum_forbidden",
    );
    assert_eq!(arg(&err, "effect"), Some("async"));
}

#[test]
fn output_is_forbidden() {
    let err = rejection(
        "fixum numerus N = praefixum {\n    nota \"x\"\n    redde 1\n}\n",
        "praefixum_forbidden",
    );
    assert_eq!(arg(&err, "effect"), Some("scribe"));
}

#[test]
fn self_reference_is_a_cycle() {
    assert_rejects_with_issue_la(
        "fixum numerus A = praefixum {\n    redde A + 1\n}\n",
        "constant_cycle",
    );
}

#[test]
fn cycle_through_a_callee_is_rejected() {
    assert_rejects_with_issue_la(
        "functio lege_a() → numerus {\n    redde A\n}\n\
         fixum numerus A = praefixum {\n    redde lege_a() + 1\n}\n",
        "constant_cycle",
    );
}

#[test]
fn evaluation_order_follows_static_reads() {
    let fixture = analyze_source_la(
        "fixum numerus B = praefixum {\n    redde A * 2\n}\n\
         fixum numerus C = A + 1\n\
         fixum numerus A = praefixum {\n    redde 3\n}\n",
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let order = evaluation_order(hir, &fixture.result.types).expect("no cycle");
    let names: Vec<&str> = order
        .iter()
        .map(|site| fixture.interner.resolve(site.name))
        .collect();
    assert_eq!(names, ["A", "B"]);
}

// ST2-2.1: import references.

mod imported {
    use super::super::praefixum::{
        ImportedCallee, ImportedEffects, ImportedReference, ImportedReferenceKind, function_effect,
        imported_references,
    };
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceConstValue,
        InterfaceTypeSnapshot,
    };
    use crate::passes::visibility_annotation::VisibilityTier::Publica;
    use crate::test_support::{AnalyzedFixture, analyze_source_with_import_contract_la};
    use crate::{DefId, Primitive};
    use radix_hir::{HirItemKind, HirModule};
    use radix_lexer::Interner;
    use std::collections::BTreeMap;

    fn analyze(source: &str) -> AnalyzedFixture {
        let mut interface = FileInterface::new();
        interface.insert(FileExport {
            name: "K".to_owned(),
            kind: FileExportKind::Const(
                InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                Some(InterfaceConstValue::Int(5)),
            ),
        });
        for name in ["f", "g"] {
            interface.insert(FileExport {
                name: name.to_owned(),
                kind: FileExportKind::Function(InterfaceCallable {
                    type_params: Vec::new(),
                    params: Vec::new(),
                    ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
                    err: None,
                    is_async: false,
                    is_generator: false,
                    tensor_edge_policy_requirements: Vec::new(),
                    device: None,
                }),
            });
        }
        let exports = ["K", "f", "g"]
            .into_iter()
            .map(|name| (name.to_owned(), Publica))
            .collect();
        let fixture = analyze_source_with_import_contract_la(
            source,
            BTreeMap::from([("ns".to_owned(), exports)]),
            BTreeMap::from([("ns".to_owned(), interface)]),
        );
        assert!(
            fixture.is_ok(),
            "diagnostics: {:?} in {source}",
            fixture.diagnostics
        );
        fixture
    }

    fn def_named(fixture: &AnalyzedFixture, hir: &HirModule, wanted: &str) -> DefId {
        hir.items
            .iter()
            .find_map(|item| match &item.kind {
                HirItemKind::Function(function)
                    if fixture.interner.resolve(function.name) == wanted =>
                {
                    Some(item.def_id)
                }
                HirItemKind::Constant(konst) if fixture.interner.resolve(konst.name) == wanted => {
                    Some(item.def_id)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no item named {wanted}"))
    }

    fn references(fixture: &AnalyzedFixture, wanted: &str) -> Vec<ImportedReference> {
        let hir = fixture.result.hir.as_ref().expect("hir");
        let site = def_named(fixture, hir, wanted);
        imported_references(hir, &fixture.result.types, site)
    }

    const IMPORT: &str = "importa ex \"./ns\" ns\n";

    #[test]
    fn namespace_call_is_one_call_reference() {
        let fixture = analyze(&format!(
            "{IMPORT}fixum numerus A = praefixum {{\n    redde ns.f()\n}}\n"
        ));
        let found = references(&fixture, "A");
        assert_eq!(found.len(), 1, "references: {found:?}");
        assert_eq!(found[0].kind, ImportedReferenceKind::Call);
        assert_eq!(fixture.interner.resolve(found[0].member), "f");
    }

    #[test]
    fn namespace_value_is_one_value_reference() {
        let fixture = analyze(&format!(
            "{IMPORT}fixum numerus A = praefixum {{\n    redde ns.K\n}}\n"
        ));
        let found = references(&fixture, "A");
        assert_eq!(found.len(), 1, "references: {found:?}");
        assert_eq!(found[0].kind, ImportedReferenceKind::Value);
        assert_eq!(fixture.interner.resolve(found[0].member), "K");
    }

    #[test]
    fn reference_through_a_local_helper_is_reported() {
        let fixture = analyze(&format!(
            "{IMPORT}functio helper() → numerus {{\n    redde ns.f()\n}}\n\
             fixum numerus A = praefixum {{\n    redde helper()\n}}\n"
        ));
        let found = references(&fixture, "A");
        assert_eq!(found.len(), 1, "references: {found:?}");
        assert_eq!(found[0].kind, ImportedReferenceKind::Call);
    }

    #[test]
    fn body_without_import_references_reports_none() {
        let fixture = analyze(&format!(
            "{IMPORT}functio helper() → numerus {{\n    redde ns.f()\n}}\n\
             fixum numerus A = praefixum {{\n    redde 1 + 2\n}}\n"
        ));
        assert_eq!(
            references(&fixture, "A"),
            [] as [crate::passes::praefixum::ImportedReference; 0]
        );
    }

    #[test]
    fn local_method_call_is_not_an_import_reference() {
        let fixture = analyze(&format!(
            "{IMPORT}genus Cista {{\n    varia numerus n\n    functio ter() → numerus {{\n        \
             redde 3\n    }}\n}}\n\
             functio sume(Cista c) → numerus {{\n    redde c.ter()\n}}\n"
        ));
        assert_eq!(
            references(&fixture, "sume"),
            [] as [crate::passes::praefixum::ImportedReference; 0]
        );
    }

    /// Answers `Effect { "scribe" }` for the member `g`, `Pure` otherwise.
    struct Stub<'a>(&'a Interner);

    impl ImportedEffects for Stub<'_> {
        fn callee(&mut self, reference: &ImportedReference) -> ImportedCallee {
            if self.0.resolve(reference.member) == "g" {
                ImportedCallee::Effect {
                    effect: "scribe",
                    chain: vec!["g".to_owned()],
                }
            } else {
                ImportedCallee::Pure
            }
        }
    }

    #[test]
    fn function_effect_spells_an_imported_hop() {
        let fixture = analyze(&format!(
            "{IMPORT}functio f() → numerus {{\n    redde ns.g()\n}}\n"
        ));
        let hir = fixture.result.hir.as_ref().expect("hir");
        let root = def_named(&fixture, hir, "f");
        let found = function_effect(
            hir,
            &fixture.result.types,
            &fixture.interner,
            root,
            &mut Stub(&fixture.interner),
        );
        assert_eq!(
            found,
            ImportedCallee::Effect {
                effect: "scribe",
                chain: vec!["f".to_owned(), "ns.g".to_owned()],
            }
        );
    }

    #[test]
    fn function_effect_keeps_local_hops_before_the_import() {
        let fixture = analyze(&format!(
            "{IMPORT}functio inner() → numerus {{\n    redde ns.g()\n}}\n\
             functio f() → numerus {{\n    redde inner()\n}}\n"
        ));
        let hir = fixture.result.hir.as_ref().expect("hir");
        let root = def_named(&fixture, hir, "f");
        let found = function_effect(
            hir,
            &fixture.result.types,
            &fixture.interner,
            root,
            &mut Stub(&fixture.interner),
        );
        assert_eq!(
            found,
            ImportedCallee::Effect {
                effect: "scribe",
                chain: vec!["f".to_owned(), "inner".to_owned(), "ns.g".to_owned()],
            }
        );
    }

    #[test]
    fn unavailable_import_is_not_pure() {
        struct Blind;
        impl ImportedEffects for Blind {
            fn callee(&mut self, _: &ImportedReference) -> ImportedCallee {
                ImportedCallee::Unavailable
            }
        }
        let fixture = analyze(&format!(
            "{IMPORT}functio f() → numerus {{\n    redde ns.f()\n}}\n"
        ));
        let hir = fixture.result.hir.as_ref().expect("hir");
        let root = def_named(&fixture, hir, "f");
        let found = function_effect(
            hir,
            &fixture.result.types,
            &fixture.interner,
            root,
            &mut Blind,
        );
        assert_eq!(found, ImportedCallee::Unavailable);
    }
}
