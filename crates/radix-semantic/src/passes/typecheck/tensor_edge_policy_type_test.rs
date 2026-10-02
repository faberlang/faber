//! Builtin tensor edge policies are type-only selectors, not value types.

fn analyze_in_locale(source: &str, locale: &str) -> crate::test_support::AnalyzedFixture {
    let pack = crate::locale::LocalePack::from_toml_path(
        crate::locale::workspace_locale_pack_path(locale),
    )
    .expect("locale pack");
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(pack);
    crate::test_support::analyze_source_with_config(source, config)
}

#[test]
fn builtin_edge_policy_marker_cannot_be_stored_as_a_local_value() {
    let result = analyze_in_locale(
        r"incipit {
    fixum clampa policy
}",
        "la",
    );
    assert!(
        !result.is_ok(),
        "edge-policy markers must not become runtime storage types"
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.is_error()
                && diagnostic.issue() == Some("tensor_edge_policy_marker_not_runtime_value")
        }),
        "expected explicit marker value-use rejection: {:?}",
        result.diagnostics
    );
}

#[test]
fn localized_edge_policy_type_names_bind_to_the_same_builtin_identity() {
    use crate::BuiltinTensorEdgePolicy;
    use crate::locale::LocalePack;
    use radix_target::Target;

    let english_pack = LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("English locale pack");
    let latin_pack = LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("Latin locale pack");
    assert_eq!(
        english_pack.type_binding_kind("clamp"),
        Some(crate::locale::TypeBindingKind::TensorEdgePolicy(
            BuiltinTensorEdgePolicy::Clamp
        ))
    );
    assert_eq!(
        latin_pack.type_binding_kind("clampa"),
        Some(crate::locale::TypeBindingKind::TensorEdgePolicy(
            BuiltinTensorEdgePolicy::Clamp
        ))
    );
    assert_eq!(english_pack.type_binding_kind("UserClamp"), None);

    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(english_pack);
    let result = crate::test_support::analyze_source_with_config(
        r"main {
    const clamp policy
}",
        config,
    );
    assert!(
        !result.is_ok(),
        "English builtin spelling also denotes a non-value marker"
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.is_error()
                && diagnostic.issue() == Some("tensor_edge_policy_marker_not_runtime_value")
        }),
        "expected localized marker value-use rejection: {:?}",
        result.diagnostics
    );
}
