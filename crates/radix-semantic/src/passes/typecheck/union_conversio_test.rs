//! CTR-04 semantic contract for standalone `@ conversio` function enrollment.
//!
//! The in-union arm form is retracted (CTR-03); the only elevated source form
//! is a named ordinary function whose marker derives one ordered concrete
//! `(source → destination)` row in the coherence registry. Both explicit
//! conversion and failable-call lookup consult that one registry.

use super::test_support;
use crate::scope::ConversionPair;
use crate::{Primitive, Resolver, TypeTable};
use radix_hir::{DefId, HirItemKind};

const VALID_ROW: &str = "\
discretio Sourceum { Failed }
discretio Destinatio { Converted }
@ conversio
functio ex_source(Sourceum err) → Destinatio { redde finge Converted {} ∷ Destinatio }
";

fn row_pair(fixture: &test_support::AnalyzedFixture) -> (crate::TypeId, crate::TypeId, DefId) {
    let hir = fixture.hir.as_ref().expect("typed HIR");
    hir.items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Function(function)
                if fixture.interner.resolve(function.name) == "ex_source" =>
            {
                Some((
                    function.params[0].ty,
                    function.ret_ty.expect("ret"),
                    item.def_id,
                ))
            }
            _ => None,
        })
        .expect("marked function")
}

#[test]
fn marked_function_is_callable_by_name_and_enrolls_one_row() {
    let fixture = test_support::analyze_source_la(&format!(
        "{VALID_ROW}\
functio user() → Destinatio {{ redde ex_source(finge Failed {{}} ∷ Sourceum) }}
"
    ));
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let (source, destination, fn_def_id) = row_pair(&fixture);
    let row = fixture
        .resolver
        .conversion_row_for(source, destination)
        .expect("enrolled row");
    assert_eq!(row.fn_def_id, Some(fn_def_id));
    assert!(!row.builtin, "user row is not a builtin row");
    // Enrolled exactly once: the same pair resolves the same row identity.
    assert_eq!(
        fixture
            .resolver
            .conversion_rows_for(ConversionPair::new(source, destination))
            .len(),
        1
    );
}

#[test]
fn failable_call_propagation_resolves_the_enrolled_row() {
    let fixture = test_support::analyze_source_la(&format!(
        "{VALID_ROW}\
functio source() → vacuum ⇥ Sourceum {{}}
functio target() → vacuum ⇥ Destinatio {{ redde source() }}
"
    ));
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
}

#[test]
fn explicit_conversio_resolves_the_same_row() {
    let fixture = test_support::analyze_source_la(&format!(
        "{VALID_ROW}\
incipit {{ sit Destinatio d ← finge Failed {{}} ∷ Sourceum ↦ Destinatio }}
"
    ));
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
}

#[test]
fn self_pair_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        "discretio Sourceum { Failed }\n@ conversio\nfunctio f(Sourceum err) → Sourceum { redde err }",
        "conversio_self_pair",
    );
}

#[test]
fn generic_helper_cannot_key_a_row() {
    test_support::assert_rejects_with_issue_la(
        "discretio Sourceum { Failed }\ndiscretio Destinatio { Converted }\n@ conversio\nfunctio f<T>(T err) → Destinatio { redde finge Converted {} ∷ Destinatio }",
        "conversio_generic_row",
    );
}

#[test]
fn signature_must_derive_one_ordered_pair() {
    test_support::assert_rejects_with_issue_la(
        "discretio Sourceum { Failed }\ndiscretio Destinatio { Converted }\n@ conversio\nfunctio f(Sourceum a, Sourceum b) → Destinatio { redde finge Converted {} ∷ Destinatio }",
        "conversio_signature",
    );
    test_support::assert_rejects_with_issue_la(
        "discretio Sourceum { Failed }\ndiscretio Destinatio { Converted }\n@ conversio\nfunctio f(Sourceum err) { redde err }",
        "conversio_signature",
    );
}

#[test]
fn impure_body_fails_closed() {
    test_support::assert_rejects_with_issue_la(
        "discretio Sourceum { Failed }\ndiscretio Destinatio { Converted }\n@ conversio\nfunctio f(Sourceum err) → Destinatio { varia textus t ← \"x\"  t ← \"y\"  redde finge Converted {} ∷ Destinatio }",
        "conversio_impure_body",
    );
}

#[test]
fn duplicate_pair_carries_dual_spans() {
    let fixture = test_support::analyze_source_la(
        "discretio Sourceum { Failed }\ndiscretio Destinatio { Converted }\n@ conversio\nfunctio first(Sourceum err) → Destinatio { redde finge Converted {} ∷ Destinatio }\n@ conversio\nfunctio second(Sourceum err) → Destinatio { redde finge Converted {} ∷ Destinatio }",
    );
    let error = fixture
        .diagnostics
        .iter()
        .find(|error| error.issue() == Some("conversio_registry_duplicate_pair"))
        .unwrap_or_else(|| panic!("duplicate diagnostic: {:?}", fixture.diagnostics));
    assert!(error.span.start < error.span.end);
    assert!(error.args.iter().any(|arg| arg.name == "first_span"));
    assert!(error.args.iter().any(|arg| arg.name == "second_span"));
}

#[test]
fn user_row_cannot_shadow_a_seeded_builtin_row() {
    test_support::assert_rejects_with_issue_la(
        "@ conversio\nfunctio f(textus s) → octeti { redde s ↦ octeti }",
        "conversio_registry_builtin_shadow",
    );
}

#[test]
fn competing_named_helpers_remain_legal() {
    let fixture = test_support::analyze_source_la(
        "discretio Sourceum { Failed }\ndiscretio Destinatio { Converted }\n@ conversio\nfunctio chosen(Sourceum err) → Destinatio { redde finge Converted {} ∷ Destinatio }\nfunctio rival(Sourceum err) → Destinatio { redde finge Converted {} ∷ Destinatio }",
    );
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
}

#[test]
fn transitive_chain_stays_fail_closed_without_a_direct_row() {
    let fixture = test_support::analyze_source_la(
        r"
discretio A { Failed }
discretio B { FromA }
discretio C { FromB }
@ conversio
functio a_to_b(A err) → B { redde finge FromA {} ∷ B }
@ conversio
functio b_to_c(B err) → C { redde finge FromB {} ∷ C }
functio source() → vacuum ⇥ A {}
functio target() → vacuum ⇥ C { redde source() }
",
    );
    let error = fixture
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.issue() == Some("failable_call_conversion_absent"))
        .expect("direct conversion absence diagnostic");
    assert!(
        error
            .args
            .iter()
            .any(|arg| arg.name == "source" && arg.value == "A")
    );
    assert!(
        error
            .args
            .iter()
            .any(|arg| arg.name == "destination" && arg.value == "C")
    );
    assert!(
        !error.args.iter().any(|arg| arg.name == "suggestion"),
        "no named helper exists for A → C"
    );
}

#[test]
fn absent_direct_row_suggests_a_named_helper_when_available() {
    let fixture = test_support::analyze_source_la(
        r"
discretio A { Failed }
discretio B { FromA }
discretio C { FromB }
@ conversio
functio a_to_c(A err) → C { redde finge FromB {} ∷ C }
@ conversio
functio b_to_c(B err) → C { redde finge FromB {} ∷ C }
functio source() → vacuum ⇥ B {}
functio target() → vacuum ⇥ C { redde source() }
",
    );
    // No visible row B → C exists outside the suggestion table? Both rows are
    // module-scope visible here, so the call admits; the suggestion surface is
    // proven below against a hand-built registry instead.
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
}

#[test]
fn conversion_name_suggestion_finds_an_out_of_scope_helper() {
    let mut resolver = Resolver::new();
    let types = TypeTable::new();
    let source = types.primitive(Primitive::Textus);
    let destination = types.primitive(Primitive::Numerus);
    let fn_id = DefId(900);
    let mut interner = radix_lexer::Interner::new();
    let helper_name = interner.intern("helper");
    let _ = resolver.define(crate::Symbol {
        def_id: fn_id,
        name: helper_name,
        kind: crate::SymbolKind::Function,
        ty: None,
        mutable: false,
        span: crate::test_support::test_span(),
    });
    resolver.enter_scope(crate::ScopeKind::Module);
    resolver
        .register_user_conversion(
            ConversionPair::new(source, destination),
            fn_id,
            crate::test_support::test_span(),
        )
        .expect("unique enrollment");
    resolver.exit_scope();
    // Back at global scope the direct lookup misses; the name is still
    // suggestible.
    assert!(resolver.conversion_row_for(source, destination).is_none());
    assert!(
        resolver
            .conversion_name_suggestion(source, destination)
            .is_some()
    );
}

#[test]
fn orphan_ownership_rejects_both_ends_foreign() {
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let a = types.intern(crate::Type::Enum(DefId(700)));
    let b = types.intern(crate::Type::Enum(DefId(701)));
    let c = types.intern(crate::Type::Enum(DefId(702)));
    resolver.note_imported_type_def(DefId(700));
    resolver.note_imported_type_def(DefId(701));
    let orphan = crate::lower::decl::conversion_pair_is_orphan;
    assert!(orphan(&resolver, &types, a, b), "both ends foreign");
    assert!(!orphan(&resolver, &types, a, c), "one end owned");
    let textus = types.primitive(Primitive::Textus);
    assert!(
        !orphan(&resolver, &types, a, textus),
        "compiler-shipped carrier is not foreign"
    );
}

#[test]
fn builtin_rows_are_seeded_at_analysis_setup() {
    let mut fixture = test_support::analyze_source_la("incipit { }");
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let textus = fixture.types.primitive(Primitive::Textus);
    let octeti = fixture.types.octeti_list();
    for (source, destination) in [(textus, octeti), (octeti, textus)] {
        let row = fixture
            .resolver
            .conversion_row_for(source, destination)
            .expect("seeded builtin row consultable at analysis setup");
        assert!(row.builtin, "seeded row is compiler-shipped");
        assert_eq!(row.fn_def_id, None, "builtin rows carry no user function");
    }
}

#[test]
fn local_handler_keeps_original_error_type_and_does_not_convert() {
    let fixture = test_support::analyze_source_la(&format!(
        "{VALID_ROW}\
functio source() → vacuum ⇥ Sourceum {{}}
functio target() → vacuum ⇥ Destinatio {{
    fac {{ source() }}
    cape err {{}}
}}
"
    ));
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
}

#[test]
fn non_call_sinks_still_reject_mismatched_error_types() {
    for source in [
        format!(
            "{VALID_ROW}\
functio source() → vacuum ⇥ Sourceum {{}}\n\
functio target() → vacuum ⇥ Destinatio {{ iace source() }}"
        ),
        format!(
            "{VALID_ROW}\
functio source() → vacuum ⇥ Sourceum {{}}\n\
functio target() → vacuum ⇥ Destinatio {{ requirit falsum iace source() }}"
        ),
    ] {
        let fixture = test_support::analyze_source_la(&source);
        assert!(!fixture.success(), "expected sink mismatch to reject");
        assert!(
            fixture
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.issue() != Some("failable_call_conversion_absent")),
            "non-call sink consulted conversion registry: {:?}",
            fixture.diagnostics
        );
    }
}
