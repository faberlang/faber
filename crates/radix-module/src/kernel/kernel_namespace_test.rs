//! Residual `faber:*` prefix: N2 deleted the kernel modules.

use super::{
    KernelModule, KernelResolveError, is_kernel_import_path, kernel_manifest_entries, kernel_module_count,
    resolve_kernel_module,
};
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::mir::{MirProvider, MirProviderKind};

#[test]
fn kernel_prefix_detects_faber_imports() {
    assert!(is_kernel_import_path("faber:solum"));
    assert!(is_kernel_import_path("faber:json"));
    assert!(!is_kernel_import_path("norma:json"));
}

#[test]
fn manifest_lists_no_kernel_modules() {
    assert_eq!(kernel_module_count(), 0);
    assert!(kernel_manifest_entries().is_empty());
}

#[test]
fn unknown_kernel_module_fails_with_kernel_diagnostic() {
    let err = resolve_kernel_module("faber:nihil").unwrap_err();
    assert_eq!(err, KernelResolveError::UnknownModule { name: "nihil".to_owned() });
}

#[test]
fn nested_faber_paths_are_invalid() {
    let err = resolve_kernel_module("faber:hal/solum").unwrap_err();
    assert!(matches!(err, KernelResolveError::InvalidPath { .. }));
}

#[test]
fn former_kernel_import_is_unknown() {
    let err = resolve_kernel_module("faber:processus").unwrap_err();
    assert_eq!(err, KernelResolveError::UnknownModule { name: "processus".to_owned() });
    let err = resolve_kernel_module("faber:json").unwrap_err();
    assert_eq!(err, KernelResolveError::UnknownModule { name: "json".to_owned() });
}

#[test]
fn kernel_glob_import_rejects_unknown_module_early() {
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let source = r#"
importa ex "faber:*" faber

incipit {
    nota faber.missing.foo()
}
"#;
    let Err(diagnostics) = analyze_source(&session, "kernel-glob-import.fab", source, AnalyzeRequest::default()) else {
        panic!("analysis should fail")
    };
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "kernel_unknown_module")
                && diagnostic
                    .args
                    .iter()
                    .any(|arg| arg.name == "module" && arg.value == "missing")
        }),
        "expected unknown module diagnostic, got {diagnostics:?}"
    );
}

#[test]
fn mir_provider_kernel_discriminator_round_trips_module() {
    for module in [
        KernelModule::Solum,
        KernelModule::Processus,
        KernelModule::Aleator,
        KernelModule::Json,
        KernelModule::Consolum,
        KernelModule::Toml,
    ] {
        let provider =
            MirProvider { kind: MirProviderKind::Kernel(module), module: Vec::new(), name: crate::lexer::Symbol(0) };
        assert_eq!(provider.kind, MirProviderKind::Kernel(module));
    }
}
