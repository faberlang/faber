//! FMIR package targets are not a radix single-file emit path.
//!
//! Radix must reject `fmir-text` / `fmir` / `fmir-bin` with CODEGEN001 (delegate
//! to `faber build`). Package-shaped FMIR proofs live under `faber/corpus` and
//! are exercised by exempla (`tensor_package_*`), not here.

use crate::codegen::Target;
use crate::program::compile_cli_source_with_locale_pack;

/// Minimal single-file unit — no package graph, no faber.toml, no sibling fixtures.
const SMOKE: &str = r#"
incipit {
    nota "fmir-delegation"
}
"#;

#[test]
fn fmir_targets_rejected_with_codegen001_on_single_file() {
    for target in [Target::MirFmir, Target::MirFmirBinary, Target::MirFmirBundle] {
        let result = compile_cli_source_with_locale_pack(
            "smoke.fab",
            SMOKE,
            target,
            Some(&crate::locale::latin_locale_pack()),
            crate::codegen::OutputMode::Application,
            None,
        );
        assert!(!result.success(), "{target:?} must be rejected by radix (no Rust fallback)");
        let has_delegation = result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("CODEGEN001"));
        assert!(
            has_delegation,
            "{target:?} must produce CODEGEN001 delegation diagnostic, got: {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| (d.code, d.issue()))
                .collect::<Vec<_>>()
        );
    }
}
