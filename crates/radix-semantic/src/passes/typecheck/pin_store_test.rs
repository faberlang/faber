//! Z2-P: opaque-return binding-site pin store checks.
//!
//! `fn load(string path) → tf32[_, _]` returns per-call opaque axes. A
//! declaration store `let tf32[H, W] elev ← load(path)` pins those opaque
//! axes to the annotation's named size terms at the store: the check is
//! green and the extent equality is recorded as a runtime-recoverable store
//! obligation. Literal mismatches stay red.

use crate::PassConfig;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config};
use radix_target::Target;

fn analyze(source: &str) -> AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("English test surface"),
    );
    analyze_source_with_config(source, config)
}

fn accepts(source: &str) {
    let result = analyze(source);
    assert!(
        result.is_ok(),
        "diagnostics: {:?}\n{source}",
        result.diagnostics
    );
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = analyze(source);
    assert!(!result.is_ok(), "unexpected acceptance:\n{source}");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && err.issue() == Some(issue)),
        "expected issue {issue:?}, got {:?}\n{source}",
        result.diagnostics
    );
}

const LOAD_FN: &str = "fn load(string path) → tf32[_, _] {
    let tf32[2, 3] t ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ tf32[2, 3]
    return t
}
";

#[test]
fn pin_store_opaque_axes_pin_to_named_sizes_green() {
    accepts(&format!(
        "{LOAD_FN}main {{
    const size H ← 2
    const size W ← 3
    let tf32[H, W] elev ← load(\"x\")
    print elev.shape().get(0)
    print elev.shape().get(1)
}}"
    ));
}

#[test]
fn pin_store_literal_mismatch_initializer_stays_red() {
    rejects_with_issue(
        &format!(
            "{LOAD_FN}main {{
    let tf32[2, 3] t ← seed_return()
    let tf32[3, 2] x ← t
}}

fn seed_return() → tf32[2, 3] {{
    let tf32[2, 3] t ← [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] ↦ tf32[2, 3]
    return t
}}"
        ),
        "incompatible_tensor_index",
    );
}

#[test]
fn pin_store_opaque_into_infer_annotation_stays_green() {
    accepts(&format!(
        "{LOAD_FN}main {{
    let tf32[_, _] a ← load(\"x\")
    print a.shape().get(0)
    print a.shape().get(1)
}}"
    ));
}
