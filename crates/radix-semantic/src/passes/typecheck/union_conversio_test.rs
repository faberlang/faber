//! E1 semantic contract for declaration-owned error-conversion arms.

use super::test_support;
use crate::{ConversionArm, PassConfig, Resolver, Type, TypeId};
use radix_hir::{DefId, HirExpressionKind, HirItemKind, HirStatementKind};
use radix_lexer::Span;

#[test]
fn self_pair_is_rejected() {
    test_support::assert_rejects_with_issue(
        "discretio Errorum { @ conversio Errorum self {} Failed }",
        "union_conversio_self_pair",
    );
}

#[test]
fn generic_destination_is_rejected() {
    test_support::assert_rejects_with_issue(
        "discretio Sourceum { Failed }\ndiscretio Destinatio<T> { @ conversio Sourceum {} Failed }",
        "union_conversio_generic_union",
    );
}

#[test]
fn generic_source_is_rejected() {
    test_support::assert_rejects_with_issue(
        "discretio Sourceum<T> { Failed }\ndiscretio Destinatio { @ conversio Sourceum<numerus> {} Failed }",
        "union_conversio_generic_union",
    );
}

#[test]
fn duplicate_source_pair_is_rejected_with_a_dual_span_fact() {
    let fixture = test_support::analyze_source(
        "discretio Sourceum { Failed }\ndiscretio Destinatio { @ conversio Sourceum {} @ conversio Sourceum {} Failed }",
    );
    let error = fixture
        .diagnostics
        .iter()
        .find(|error| error.issue() == Some("union_conversio_duplicate_pair"))
        .unwrap_or_else(|| panic!("duplicate pair diagnostic: {:?}", fixture.diagnostics));
    assert!(error.span.start < error.span.end);
    assert!(error.args.iter().any(|arg| arg.name == "first_span"));
    assert!(error.args.iter().any(|arg| arg.name == "second_span"));
}

#[test]
fn conversion_annotation_is_rejected_off_a_union() {
    test_support::assert_rejects_with_issue(
        "@ conversio Sourceum {}\nfunctio f() → vacuum {}\ndiscretio Sourceum { Failed }",
        "union_conversio_misplaced",
    );
}

#[test]
fn valid_arm_lowers_to_an_enum_row_and_synthetic_function() {
    let fixture = test_support::analyze_source(
        "discretio Sourceum { Failed }\ndiscretio Destinatio { @ conversio Sourceum err { redde finge Converted {} ∷ Destinatio } Converted }",
    );
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.hir.as_ref().expect("hir");
    let destination = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Enum(enum_item)
                if fixture.interner.resolve(enum_item.name) == "Destinatio" =>
            {
                Some(enum_item)
            }
            _ => None,
        })
        .expect("destination enum");
    assert_eq!(destination.conversion_arms.len(), 1);
    assert_eq!(
        destination.conversion_arms[0]
            .self_ident
            .map(|name| fixture.interner.resolve(name)),
        Some("err")
    );
    assert!(hir.items.iter().any(|item| {
        matches!(item.kind, HirItemKind::Function(_))
            && item.def_id == destination.conversion_arms[0].fn_def_id
    }));
}

#[test]
fn la_conversio_arm_registers_with_the_installed_pack() {
    let pack_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/la/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path)
        .expect("installed la pack should load");
    // RED (F1): this row assertion fails while d2dd1c871 still declares the
    // cross-locale `conversion` alias in the Latin pack.
    assert!(
        !pack.keywords.contains_key("conversion"),
        "la must seal the English `conversion` spelling out of its pack"
    );

    let mut config = PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(pack);
    let fixture = test_support::analyze_source_with_config(
        "discretio Sourceum { Failed }\ndiscretio Destinatio { @ conversio Sourceum err { redde finge Converted {} ∷ Destinatio } Converted }",
        config,
    );
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.hir.as_ref().expect("typed HIR");
    let destination = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Enum(enum_item)
                if fixture.interner.resolve(enum_item.name) == "Destinatio" =>
            {
                Some(enum_item)
            }
            _ => None,
        })
        .expect("destination enum");
    assert_eq!(
        destination.conversion_arms.len(),
        1,
        "the la `@ conversio` arm must register in typed HIR"
    );
}

#[test]
fn resolver_registry_is_environment_scoped_for_cross_module_lookup() {
    let mut resolver = Resolver::new();
    resolver.register_conversion_arms(
        DefId(700),
        vec![ConversionArm {
            source: TypeId(17),
            fn_def_id: DefId(900),
            span: Span::default(),
        }],
    );
    assert_eq!(
        resolver.conversion_arm_for(DefId(700), TypeId(17)),
        Some(DefId(900))
    );
}

#[test]
fn failable_call_propagation_records_declared_conversion_arm() {
    let fixture = test_support::analyze_source(
        r#"
discretio Sourceum { Failed }
discretio Destinatio { @ conversio Sourceum err { redde finge Converted {} ∷ Destinatio } Converted }
functio source() → vacuum ⇥ Sourceum {}
functio target() → vacuum ⇥ Destinatio { redde source() }
"#,
    );
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.hir.as_ref().expect("typed HIR");
    let destination_arm = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Enum(enm) if fixture.interner.resolve(enm.name) == "Destinatio" => {
                Some(enm.conversion_arms[0].fn_def_id)
            }
            _ => None,
        })
        .expect("destination conversion arm");
    let target_body = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Function(function)
                if fixture.interner.resolve(function.name) == "target" =>
            {
                function.body.as_ref()
            }
            _ => None,
        })
        .expect("target body");
    let call = match &target_body.statements[0].kind {
        HirStatementKind::Redde(Some(expr)) => expr,
        other => panic!("expected redde call, got {other:?}"),
    };
    assert!(matches!(
        call.kind,
        HirExpressionKind::Call(_, _, _, Some(arm)) if arm == destination_arm
    ));
}

#[test]
fn transitive_conversion_chain_is_fail_closed_without_direct_arm() {
    let fixture = test_support::analyze_source(
        r#"
discretio A { Failed }
discretio B { @ conversio A err { redde finge FromA {} ∷ B } FromA }
discretio C { @ conversio B err { redde finge FromB {} ∷ C } FromB }
functio source() → vacuum ⇥ A {}
functio target() → vacuum ⇥ C { redde source() }
"#,
    );
    let error = fixture
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.issue() == Some("failable_call_conversion_absent"))
        .expect("direct conversion absence diagnostic");
    assert!(error
        .args
        .iter()
        .any(|arg| arg.name == "source" && arg.value == "A"));
    assert!(error
        .args
        .iter()
        .any(|arg| arg.name == "destination" && arg.value == "C"));
}

#[test]
fn local_handler_keeps_original_error_type_and_does_not_convert() {
    let fixture = test_support::analyze_source(
        r#"
discretio Sourceum { Failed }
discretio Destinatio { @ conversio Sourceum err { redde finge Converted {} ∷ Destinatio } Converted }
functio source() → vacuum ⇥ Sourceum {}
functio target() → vacuum ⇥ Destinatio {
    fac { source() }
    cape err {}
}
"#,
    );
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let source_def = fixture
        .hir
        .as_ref()
        .expect("HIR")
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Enum(enm) if fixture.interner.resolve(enm.name) == "Sourceum" => {
                Some(item.def_id)
            }
            _ => None,
        })
        .expect("source enum");
    let target_body = fixture
        .hir
        .as_ref()
        .expect("HIR")
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Function(function)
                if fixture.interner.resolve(function.name) == "target" =>
            {
                function.body.as_ref()
            }
            _ => None,
        })
        .expect("target body");
    let handled = match &target_body.statements[0].kind {
        HirStatementKind::Expr(expr) => expr,
        other => panic!("expected handled expression, got {other:?}"),
    };
    let catch = match &handled.kind {
        HirExpressionKind::Handled { catch, .. } => catch,
        other => panic!("expected handled call, got {other:?}"),
    };
    assert!(matches!(
        fixture.types.get(catch.binding_ty.expect("catch type")),
        Type::Enum(def_id) if *def_id == source_def
    ));
}

#[test]
fn non_call_sinks_still_reject_mismatched_error_types() {
    for source in [
        r#"
discretio Sourceum { Failed }
discretio Destinatio { @ conversio Sourceum err { redde finge Converted {} ∷ Destinatio } Converted }
functio source() → vacuum ⇥ Sourceum {}
functio target() → vacuum ⇥ Destinatio { iace source() }
"#,
        r#"
discretio Sourceum { Failed }
discretio Destinatio { @ conversio Sourceum err { redde finge Converted {} ∷ Destinatio } Converted }
functio source() → vacuum ⇥ Sourceum {}
functio target() → vacuum ⇥ Destinatio { requirit falsum iace source() }
"#,
        r#"
discretio Sourceum { Failed }
discretio Destinatio { @ conversio Sourceum err { redde finge Converted {} ∷ Destinatio } Converted }
functio source() → vacuum ⇥ Sourceum {}
functio target() → vacuum ⇥ Destinatio {
    varia Destinatio value ← finge Converted {} ∷ Destinatio
    value ↤ source()
}
"#,
    ] {
        let fixture = test_support::analyze_source(source);
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
