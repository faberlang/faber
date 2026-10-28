//! Bounded-param PKG001 empty-message regression (goal
//! `flow-narrowing-and-match-patterns`, P2 rider).
//!
//! `fn probe(list<int, size 4> b)` is legal type grammar — the same type is green
//! in main-body position (`ct-w6`) and `numerus<u32>` params pass (`ct-w1`).
//! Package check still rejects it: the file-interface snapshot cannot yet
//! represent bounded collection types, so the exported signature fails closed
//! at the S0-G6 seam. The rejection must surface as a real, rendered
//! diagnostic — never the empty `error[PKG001]: <file>` body this suite
//! guards against.

use radix_module::diagnostics::{Diagnostic, render_plain};
use radix_module::driver::Config;

use crate::analyze::{AnalysisSeams, check_package};
use crate::discovery::PackageSpec;
use crate::library::LibraryResolver;
use std::fs;
use std::path::{Path, PathBuf};

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "radix-program-bounded-param-pkg001-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("mkdir");
    path
}

fn default_locale_pack(
    _input: &Path,
    _locale: Option<&str>,
) -> Result<Option<radix_module::locale::LocalePack>, Box<Diagnostic>> {
    let path = radix_module::locale::workspace_locale_pack_path("en");
    let pack = radix_module::locale::LocalePack::from_toml_path(path).map_err(|err| {
        Box::new(
            Diagnostic::error(format!("failed to load English locale pack: {err}"))
                .with_phase(radix_module::diagnostics::DiagnosticPhase::Lex),
        )
    })?;
    Ok(Some(pack))
}

fn accept_manifest(_spec: &PackageSpec) -> Result<(), Box<Diagnostic>> {
    Ok(())
}

fn empty_resolver_for_package(
    _config: &Config,
    _package_root: &Path,
) -> Result<LibraryResolver, Vec<Diagnostic>> {
    Ok(LibraryResolver::with_optional_home(None))
}

fn test_seams() -> AnalysisSeams {
    AnalysisSeams {
        locale_loader: default_locale_pack,
        manifest_validator: accept_manifest,
        resolver_for_package: empty_resolver_for_package,
    }
}

/// Red-first repro for the P2 empty-message PKG001: a bounded type the file
/// interface cannot represent, in a `fn` param position (`ct-w2` / `ct-w5` /
/// `ct-widths`), fails the check with a real, rendered diagnostic — not
/// `error[PKG001]: <file>`. A bounded string stands in: a sized list now
/// exports, a bounded string still does not.
#[test]
fn bounded_param_exported_signature_emits_real_rendered_diagnostic() {
    let dir = temp_root("ct-w2");
    let entry = dir.join("main.fab");
    fs::write(
        &entry,
        "@ public\nfn probe(string<size 4> b) → void {\n    print b\n}\n",
    )
    .expect("write source");

    let diagnostics = check_package(&Config::default(), &entry, &test_seams());
    let _ = fs::remove_dir_all(&dir);

    assert!(
        !diagnostics.is_empty(),
        "bounded param must still fail closed (file interface cannot represent the signature)"
    );
    assert!(
        diagnostics
            .iter()
            .any(radix_module::diagnostics::Diagnostic::is_error),
        "the bounded-param rejection must be an error: {diagnostics:?}"
    );
    for diag in &diagnostics {
        assert!(
            !diag.message.trim().is_empty(),
            "no diagnostic may carry an empty message (empty PKG001 body): {diag:?}"
        );
        let rendered = render_plain(diag);
        assert!(
            rendered.contains(diag.message.trim()),
            "the diagnostic message must render into the default `faber check` output: {rendered}"
        );
    }
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("interface_export_unrepresentable")),
        "the rejection must be machine-identifiable: {diagnostics:?}"
    );
}
