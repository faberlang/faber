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
    let result = analyze_source_with_config(
        "fn main() {\n    const size H ← 3\n    print H\n}\n",
        config,
    );
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
    let result = analyze_source_with_config(
        "fn main() {\n    const size H ← -3\n    print H\n}\n",
        config,
    );
    assert!(
        result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "size_lift_negative")),
        "expected size_lift_negative, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn static_size_folds_in_tensor_shape() {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    let result = analyze_source_with_config(
        "fn main() {\n    const size N = 3\n    const tf32[] seed ← empty\n    var tf32[N, 2] values ← seed.create(0.0, [N, 2])\n}\n",
        config,
    );
    assert!(
        result.is_ok(),
        "expected static size to fold, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn static_size_affine_extent_checks() {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    let result = analyze_source_with_config(
        "fn main() {\n    const size N = 3\n    const tf32[] seed ← empty\n    var tf32[N + 1, 2] values ← seed.create(0.0, [N + 1, 2])\n}\n",
        config,
    );
    assert!(
        result.is_ok(),
        "expected static affine extent N + 1 to fold to 4, got: {:?}",
        result.diagnostics
    );
}

// Known gap (Z1 affine store): a provably negative static affine extent
// (`const size N = 0` then `tf32[N - 1, 2]`) must raise size_lift_negative
// at this store. The static value lives only in the HIR local's init, so the
// check needs static-size value capture in lowering (lower/stmt.rs) or the
// checker store path (typecheck/generic.rs) — both outside this unit's write
// scope. Ignored, not deleted: this is the successor's falsifiable oracle.
#[ignore = "static size value never reaches the annotation term; needs lower/stmt.rs or generic.rs (out of scope)"]
#[test]
fn static_size_affine_extent_rejects_provably_negative() {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    let result = analyze_source_with_config(
        "fn main() {\n    const size N = 0\n    const tf32[] seed ← empty\n    var tf32[N - 1, 2] values ← seed.create(0.0, [N - 1, 2])\n}\n",
        config,
    );
    assert!(
        result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "size_lift_negative")),
        "expected size_lift_negative at the N - 1 store, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn runtime_size_affine_extent_checks() {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    let result = analyze_source_with_config(
        "fn probe(i32 h) {\n    const size H ← h\n    const tf32[] seed ← empty\n    var tf32[H + 1] values ← seed.create(0.0, [H + 1])\n}\n",
        config,
    );
    assert!(
        result.is_ok(),
        "expected runtime affine extent H + 1 to check, got: {:?}",
        result.diagnostics
    );
}
