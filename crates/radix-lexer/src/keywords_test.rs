//! Keyword registry coverage: uniqueness, token correspondence, and owner-based
//! identifier-like policy.
//!
//! Tests lock the contract between `keyword_specs()`, `TokenKind::is_keyword()`,
//! and `keyword_allowed_as_ident()` against registry drift (see ca50d39c).

use super::{
    keyword_allowed_as_ident, keyword_group_specs, keyword_specs, keyword_spelling_for_token,
    keyword_token_kind, lookup_keyword_spec, validate_keyword_groups, KeywordKind, KeywordOwner,
    KeywordScope,
};
use crate::TokenKind;

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
fn tokenless_rows_must_not_be_global() {
    // `Global` means reserved anywhere normal-mode keyword recognition runs —
    // exactly what `token_kind: Some` does. A tokenless row lexes as an
    // identifier and the parser claims it by spelling in its grammar slots,
    // so it must carry `Contextual` owners (or Annotation/TestOwned/Alias)
    // mirroring those slots. This is the campaign guardrail: every migrated
    // keyword must assign owners or fail here.
    for spec in keyword_specs() {
        assert!(
            spec.token_kind.is_some() || !matches!(spec.scope, KeywordScope::Global),
            "tokenless keyword '{}' must not be Global — assign Contextual owners for its grammar slots",
            spec.text
        );
    }
}

#[test]
fn every_registered_keyword_token_reports_is_keyword() {
    // WHY: keyword_or_ident (scan.rs) and the is_keyword() match (token.rs) are
    // two parallel registries. breakable-blocks shipped Meus/Tuus in scan.rs but
    // forgot is_keyword() until ca50d39c; this guards that class of drift.
    for spec in keyword_specs() {
        let Some(ref kind) = spec.token_kind else {
            continue;
        };
        assert!(
            kind.is_keyword(),
            "{kind:?} (KeywordSpec text {:?}) is registered but not returned by is_keyword()",
            spec.text,
        );
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
fn no_keyword_has_global_scope() {
    // No reserved words: every keyword is claimed in a parser-owned slot
    // (Contextual, TestOwned, Alias, or Annotation), never reserved globally.
    for spec in keyword_specs() {
        assert!(
            !matches!(spec.scope, KeywordScope::Global),
            "keyword '{}' must not be globally reserved",
            spec.text
        );
    }
}

#[test]
fn retired_statement_spellings_are_not_registered_keywords() {
    // tempta / demum / emitte / pro were retired as statement/token surfaces;
    // after the clean break they are ordinary identifiers with no keyword spec.
    for spelling in ["tempta", "demum", "emitte", "pro"] {
        assert!(
            lookup_keyword_spec(spelling).is_none(),
            "{spelling} must not be a KeywordSpec"
        );
    }
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
fn nonnihil_remains_predicate_and_member_identifier() {
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::PredicateOperator, &TokenKind::Nonnihil),
        Some("nonnihil")
    );
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::MemberIdentifier, &TokenKind::Nonnihil),
        Some("nonnihil")
    );
    assert_eq!(
        keyword_allowed_as_ident(KeywordOwner::DeclarationName, &TokenKind::Nonnihil),
        Some("nonnihil")
    );
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
    assert!(TokenKind::Interna.is_keyword());
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
        assert!(
            identity.is_keyword(),
            "{spelling} identity must report is_keyword"
        );
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
