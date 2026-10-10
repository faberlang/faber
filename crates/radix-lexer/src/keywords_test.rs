//! Keyword registry coverage: uniqueness, token correspondence, and owner-based
//! identifier-like policy.
//!
//! Tests lock registry lookup and owner-based identifier-like policy against drift.

use super::{
    KeywordKind, KeywordOwner, KeywordScope, keyword_allowed_as_ident, keyword_group_specs,
    keyword_specs, keyword_spelling_for_token, keyword_token_kind, lookup_keyword_spec,
    validate_keyword_groups,
};
use crate::TokenKind;

#[test]
fn nonnumerus_registry_row_is_tokenless_literal_context() {
    let spec = lookup_keyword_spec("nonnumerus").expect("nonnumerus should be registered");
    assert_eq!(spec.category, KeywordKind::Literal);
    assert!(spec.token_kind.is_none());
    assert!(matches!(
        spec.scope,
        KeywordScope::Contextual(owners) if owners.contains(&KeywordOwner::Literal)
    ));
}

// =============================================================================
// SFR-2 (summa-filum-reduction): `filum` registry pin
//
// `filum` now has a tokenless contextual row, so it remains a plain identifier
// outside the parser-owned expression slot.
// =============================================================================

#[test]
fn filum_has_keyword_row_after_sfr2() {
    // SFR-2 completion: the registered row carries the parser-owned surface.
    assert!(
        lookup_keyword_spec("filum").is_some(),
        "'filum' must have a KeywordSpec row after SFR-2"
    );
}

#[test]
fn filum_lexes_as_plain_identifier_in_every_position() {
    for source in [
        "filum",
        "fixum numerus filum ← 1",
        "filum(1, 2)",
        "a.filum()",
        "filum ∴ filum",
    ] {
        let result = crate::lex(source);
        assert!(
            result.success(),
            "'{source}' must lex cleanly with filum as a plain identifier, got {:?}",
            result.errors
        );
        let filum_tokens: Vec<_> = result
            .tokens
            .iter()
            .filter(|token| &source[token.span.start as usize..token.span.end as usize] == "filum")
            .collect();
        assert!(
            !filum_tokens.is_empty(),
            "'{source}' must emit a filum token"
        );
        assert!(
            filum_tokens
                .iter()
                .all(|token| matches!(token.kind, crate::TokenKind::Ident(_))),
            "'{source}': filum must lex as an identifier, got {filum_tokens:?}"
        );
        let spec = lookup_keyword_spec("filum").expect("filum registry row");
        assert!(spec.token_kind.is_none(), "filum stays tokenless");
    }
}

#[test]
fn filum_substring_law_keeps_containing_identifiers_plain() {
    // Identifiers that merely contain "filum" as a substring stay identifiers;
    // the contextual keyword claim is exact-spelling only.
    for spelling in ["filum_x", "x_filum", "profilum", "filumensis"] {
        let result = crate::lex(spelling);
        assert!(result.success(), "'{spelling}' must lex cleanly");
        assert!(
            matches!(result.tokens[0].kind, crate::TokenKind::Ident(_)),
            "'{spelling}' must lex as a plain identifier, got {:?}",
            result.tokens[0].kind
        );
    }
}

// =============================================================================
// K3-1 (reducta): op-word, head and group registry rows
//
// Every row is tokenless and contextual: the words lex as identifiers and the
// parser claims them by Latin key in their grammar slot only.
// =============================================================================

#[test]
fn reduction_words_are_tokenless_contextual_rows_with_reduction_owners() {
    let new_rows: &[(&str, KeywordOwner)] = &[
        ("reducta", KeywordOwner::ReductionHead),
        ("pro", KeywordOwner::ReductionGroup),
        ("factum", KeywordOwner::ReductionOperator),
        ("numeratio", KeywordOwner::ReductionOperator),
        ("quilibet", KeywordOwner::ReductionOperator),
        ("argmaxima", KeywordOwner::ReductionOperator),
        ("argminima", KeywordOwner::ReductionOperator),
    ];
    for &(text, owner) in new_rows {
        let spec =
            lookup_keyword_spec(text).unwrap_or_else(|| panic!("'{text}' needs a registry row"));
        assert!(spec.token_kind.is_none(), "'{text}' must be tokenless");
        let KeywordScope::Contextual(owners) = spec.scope else {
            panic!("'{text}' must be contextual");
        };
        assert_eq!(owners, [owner], "'{text}' owner");

        let result = crate::lex(text);
        assert!(result.success(), "'{text}' must lex cleanly");
        assert!(
            matches!(result.tokens[0].kind, crate::TokenKind::Ident(_)),
            "'{text}' must lex as a plain identifier, got {:?}",
            result.tokens[0].kind
        );
    }
}

#[test]
fn sort_words_are_tokenless_contextual_rows_with_sort_owners() {
    let rows: &[(&str, KeywordOwner)] = &[
        ("ordinata", KeywordOwner::SortHead),
        ("ascendens", KeywordOwner::SortDirection),
        ("descendens", KeywordOwner::SortDirection),
    ];
    for &(text, owner) in rows {
        let spec =
            lookup_keyword_spec(text).unwrap_or_else(|| panic!("'{text}' needs a registry row"));
        assert!(spec.token_kind.is_none(), "'{text}' must be tokenless");
        let KeywordScope::Contextual(owners) = spec.scope else {
            panic!("'{text}' must be contextual");
        };
        assert_eq!(owners, [owner], "'{text}' owner");

        let result = crate::lex(text);
        assert!(result.success(), "'{text}' must lex cleanly");
        assert!(
            matches!(result.tokens[0].kind, crate::TokenKind::Ident(_)),
            "'{text}' must lex as a plain identifier, got {:?}",
            result.tokens[0].kind
        );
    }
}

#[test]
fn existing_rows_gain_the_reduction_owner_and_keep_their_old_one() {
    let gained: &[(&str, KeywordOwner, KeywordOwner)] = &[
        (
            "omnia",
            KeywordOwner::DeclarationMarker,
            KeywordOwner::ReductionOperator,
        ),
        (
            "via",
            KeywordOwner::ConversionHint,
            KeywordOwner::ReductionHead,
        ),
    ];
    for &(text, old, new) in gained {
        let spec =
            lookup_keyword_spec(text).unwrap_or_else(|| panic!("'{text}' needs a registry row"));
        let KeywordScope::Contextual(owners) = spec.scope else {
            panic!("'{text}' must be contextual");
        };
        assert!(owners.contains(&old), "'{text}' keeps {old:?}");
        assert!(owners.contains(&new), "'{text}' gains {new:?}");
    }
}

#[test]
fn legacy_reduction_heads_are_reduction_operator_only() {
    // 4be221b70: `summa`, `maxima`, `minima` retired their expression-introducer owner.
    for text in ["summa", "maxima", "minima"] {
        let spec =
            lookup_keyword_spec(text).unwrap_or_else(|| panic!("'{text}' needs a registry row"));
        let KeywordScope::Contextual(owners) = spec.scope else {
            panic!("'{text}' must be contextual");
        };
        assert_eq!(owners, &[KeywordOwner::ReductionOperator], "'{text}'");
    }
}

#[test]
fn registry_texts_are_unique() {
    let mut texts = std::collections::BTreeSet::new();

    for spec in keyword_specs() {
        assert!(
            texts.insert(spec.text),
            "duplicate KeywordSpec for '{}'",
            spec.text
        );
    }
}

#[test]
fn every_registered_keyword_token_round_trips_through_registry() {
    for spec in keyword_specs() {
        let Some(kind) = &spec.token_kind else {
            continue;
        };
        assert_eq!(keyword_token_kind(spec.text).as_ref(), Some(kind));
        let spelling = keyword_spelling_for_token(kind).expect("registered token spelling");
        assert_eq!(keyword_token_kind(spelling).as_ref(), Some(kind));
    }
}

#[test]
fn token_spelling_lookup_uses_registry_text() {
    assert_eq!(keyword_spelling_for_token(&TokenKind::Cape), Some("cape"));
    assert_eq!(
        keyword_spelling_for_token(&TokenKind::Ident(crate::Symbol(0))),
        None
    );
}

#[test]
fn contextual_keywords_are_identifier_like_only_for_owners() {
    // Structural/modifier owners keep per-keyword gating.
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::ParameterMode, &TokenKind::De),
        Some("de")
    );
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::ParameterMode, &TokenKind::Own),
        Some("own")
    );
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::ParameterMode, &TokenKind::Copy),
        Some("copy")
    );
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::FunctionModifier, &TokenKind::De),
        None
    );

    // Name-slot owners accept every keyword spelling, including those
    // whose Contextual list does not name that owner.
    for owner in [
        KeywordOwner::DeclarationName,
        KeywordOwner::ParameterName,
        KeywordOwner::MemberIdentifier,
        KeywordOwner::BindingTarget,
    ] {
        assert_eq!(keyword_allowed_as_ident(owner, &TokenKind::De), Some("de"));
        assert_eq!(
            keyword_allowed_as_ident(owner, &TokenKind::Own),
            Some("own")
        );
        assert_eq!(
            keyword_allowed_as_ident(owner, &TokenKind::Copy),
            Some("copy")
        );
        assert_eq!(
            keyword_allowed_as_ident(owner, &TokenKind::Lege),
            Some("lege")
        );
        assert_eq!(
            keyword_allowed_as_ident(owner, &TokenKind::Nota),
            Some("nota")
        );
    }

    // TypeName stays out of the name-slot policy.
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::TypeName, &TokenKind::Lege),
        None
    );
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::TypeName, &TokenKind::Iuncta),
        Some("iuncta")
    );
}

#[test]
fn annotation_names_are_owned_by_annotation_policy() {
    for (kind, spelling) in [
        (TokenKind::Metior, "metior"),
        (TokenKind::Omitte, "omitte"),
        (TokenKind::Solum, "solum"),
        (TokenKind::Tag, "tag"),
    ] {
        assert_eq!(
            keyword_allowed_as_ident(KeywordOwner::AnnotationName, &kind),
            Some(spelling)
        );
        assert_eq!(
            keyword_allowed_as_ident(KeywordOwner::DeclarationName, &kind),
            Some(spelling)
        );
    }

    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::AnnotationName, &TokenKind::Fragilis),
        None
    );
}

#[test]
fn identifier_like_owner_policy_is_shared_across_keyword_scopes() {
    // TestOwned and Alias keywords are usable as identifiers in
    // declaration/parameter/member positions (identifier-like owners); the
    // same keywords are not allowed in structural/modifier positions.
    for owner in [
        KeywordOwner::DeclarationName,
        KeywordOwner::ParameterName,
        KeywordOwner::MemberIdentifier,
        KeywordOwner::BindingTarget,
    ] {
        assert_eq!(
            keyword_allowed_as_ident(owner, &TokenKind::Scribe),
            Some("scribe")
        );
        assert_eq!(
            keyword_allowed_as_ident(owner, &TokenKind::Fragilis),
            Some("fragilis")
        );
    }

    for owner in [
        KeywordOwner::AnnotationName,
        KeywordOwner::FunctionModifier,
        KeywordOwner::PredicateOperator,
    ] {
        assert_eq!(keyword_allowed_as_ident(owner, &TokenKind::Scribe), None);
        assert_eq!(keyword_allowed_as_ident(owner, &TokenKind::Fragilis), None);
    }
}

#[test]
fn retired_predicate_sic_and_prae_words_are_not_registry_rows() {
    // KW-1: the six retired words left the registry; they lex as identifiers.
    for word in [
        "positivum",
        "negativum",
        "nonnihil",
        "nonnulla",
        "prae",
        "sic",
    ] {
        assert!(
            lookup_keyword_spec(word).is_none(),
            "{word} must not be a keyword row"
        );
        assert_eq!(keyword_token_kind(word), None, "{word} must not be a token");
    }
}

#[test]
fn annotation_family_keywords_have_identities() {
    let futura = lookup_keyword_spec("futura").expect("futura metadata should be registered");
    assert_eq!(futura.scope, KeywordScope::Annotation);
    assert_eq!(futura.category, KeywordKind::Annotation);
    assert_eq!(futura.token_kind, Some(TokenKind::Futura));

    assert!(lookup_keyword_spec("sectio").is_none());
}

#[test]
fn interna_is_visibility_annotation_keyword() {
    // VM-U1: `interna` joins the publica/privata annotation surface as a
    // contextual keyword claiming the annotation-name slot.
    let spec = lookup_keyword_spec("interna").expect("interna must be registered");
    assert_eq!(spec.token_kind, Some(TokenKind::Interna));
    assert_eq!(spec.category, KeywordKind::Modifier);
    assert_eq!(keyword_token_kind("interna"), Some(TokenKind::Interna));
    assert_eq!(
        keyword_spelling_for_token(&TokenKind::Interna),
        Some("interna")
    );
    let KeywordScope::Contextual(owners) = spec.scope else {
        panic!("interna must be contextual");
    };
    assert!(
        owners.contains(&KeywordOwner::AnnotationName),
        "interna must claim the annotation-name slot"
    );
}

#[test]
fn keyword_groups_are_static_and_complete_for_latin_surface() {
    assert!(
        validate_keyword_groups().is_empty(),
        "static keyword groups must reference live keyword rows"
    );

    let conditional = keyword_group_specs()
        .iter()
        .find(|group| group.id == "conditional")
        .expect("conditional group");
    assert_eq!(conditional.members, &["si", "sin", "secus"]);

    let loop_group = keyword_group_specs()
        .iter()
        .find(|group| group.id == "loop")
        .expect("loop group");
    assert_eq!(
        loop_group.members,
        &["dum", "fac", "itera", "apud", "perge", "rumpe"]
    );

    let closure = keyword_group_specs()
        .iter()
        .find(|group| group.id == "closure")
        .expect("closure group");
    assert_eq!(closure.members, &["clausura", "nucleum"]);
}

#[test]
fn nucleum_is_an_annotation_identity() {
    let spec = lookup_keyword_spec("nucleum").expect("nucleum must be registered");
    assert_eq!(spec.token_kind, Some(TokenKind::Nucleum));
    assert_eq!(spec.category, KeywordKind::Annotation);
    assert_eq!(keyword_token_kind("nucleum"), Some(TokenKind::Nucleum));
}

#[test]
fn apud_is_a_control_flow_keyword_identity() {
    let spec = lookup_keyword_spec("apud").expect("apud must be registered");
    assert_eq!(spec.token_kind, Some(TokenKind::Apud));
    assert_eq!(spec.category, KeywordKind::ControlFlow);
    assert_eq!(keyword_token_kind("apud"), Some(TokenKind::Apud));
    assert_eq!(keyword_spelling_for_token(&TokenKind::Apud), Some("apud"));
    let KeywordScope::Contextual(owners) = spec.scope else {
        panic!("apud must be contextual");
    };
    assert!(owners.contains(&KeywordOwner::ControlFlow));
}

#[test]
fn census_schema_and_columna_are_declaration_keywords() {
    // S2-U1: the two relational-view declaration keywords are registered with
    // declaration-owner scope, mirroring the genus/typus rows.
    for (spelling, identity) in [
        ("schema", TokenKind::Schema),
        ("columna", TokenKind::Columna),
    ] {
        let spec = lookup_keyword_spec(spelling)
            .unwrap_or_else(|| panic!("{spelling} must be a registered keyword"));
        assert_eq!(spec.token_kind, Some(identity.clone()));
        assert_eq!(spec.category, KeywordKind::Declaration);
        assert_eq!(keyword_token_kind(spelling), Some(identity.clone()));
        assert_eq!(keyword_spelling_for_token(&identity), Some(spelling));
    }

    let schema = lookup_keyword_spec("schema").expect("schema spec");
    let KeywordScope::Contextual(owners) = schema.scope else {
        panic!("schema must be contextual");
    };
    assert!(
        owners.contains(&KeywordOwner::DeclarationIntroducer),
        "schema must claim the declaration-introducer slot"
    );
}

#[test]
fn relational_view_keyword_group_bundles_declaration_keys() {
    // S2-U1 group-completeness: `schema` and `columna` localize together.
    let group = keyword_group_specs()
        .iter()
        .find(|group| group.id == "relational_view")
        .expect("relational_view group");
    assert_eq!(group.members, &["schema", "columna"]);
    assert!(
        validate_keyword_groups().is_empty(),
        "relational_view group must reference live keyword rows"
    );
}

#[test]
fn non_keyword_token_returns_none_spelling() {
    assert_eq!(
        keyword_spelling_for_token(&TokenKind::Ident(crate::Symbol(0))),
        None
    );
    assert_eq!(keyword_spelling_for_token(&TokenKind::Integer(0)), None);
}

#[test]
fn unknown_owner_returns_none_from_allowed_as_ident() {
    // A completely unknown keyword spelling (not in registry) returns None
    // regardless of owner.
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::DeclarationName, &TokenKind::Integer(0)),
        None
    );
}

#[test]
fn keyword_group_lookup_returns_empty_for_unknown_id() {
    let unknown = keyword_group_specs()
        .iter()
        .find(|group| group.id == "nonexistent_group");
    assert!(unknown.is_none(), "unknown group id should not exist");
}
