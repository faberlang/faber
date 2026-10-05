use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::analyze_source_with_config;
use radix_target::Target;

#[test]
fn size_decl_binds_immutable_value() {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    let result =
        analyze_source_with_config("fn main() {\n    size H ← 3\n    print H\n}\n", config);
    assert!(
        result.is_ok(),
        "expected green, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn size_decl_rejects_negative_literal() {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    let result =
        analyze_source_with_config("fn main() {\n    size H ← -3\n    print H\n}\n", config);
    assert!(
        result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "size_lift_negative")),
        "expected size_lift_negative, got: {:?}",
        result.diagnostics
    );
}
