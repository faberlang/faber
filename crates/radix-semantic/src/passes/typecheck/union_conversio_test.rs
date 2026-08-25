//! E1 semantic contract for declaration-owned error-conversion arms.

use super::test_support;
use radix_hir::HirItemKind;

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
