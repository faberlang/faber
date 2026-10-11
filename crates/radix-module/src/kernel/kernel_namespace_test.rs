//! Tests for kernel namespace resolution and MIR lowering.
//! Proves: kernel prefix detection; manifest matches norma public surface;
//! kernel import lowers with provider discriminator; glob import resolves
//! multiple modules; unknown module rejected early.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use super::{
    KernelModule, KernelResolveError, is_kernel_import_path, kernel_manifest_entries, kernel_module_count,
    resolve_kernel_module,
};
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::mir::MirLoweringOptions;
use crate::mir::lower_analyzed_unit;
use crate::mir::{MirIntrinsic, MirProvider, MirProviderKind, MirStatementKind};

fn library_home() -> PathBuf {
    crate::import_resolve::in_tree_library_home().expect("<repo>/lib exists in the tree")
}

fn norma_module_public_functions(module: &str) -> HashSet<String> {
    // The kernel manifest speaks the manifest's short module identities
    // (solum/processus/…); norma's English rename (norma 052ee66) moved the
    // source files to English coordinates. Resolve the manifest identity to
    // the live norma filename before reading the public surface.
    let norma_coordinate = match module {
        "solum" => "fs",
        "processus" => "process",
        "aleator" => "random",
        "consolum" => "console",
        other => other,
    };
    let path = library_home()
        .join("norma/src")
        .join(format!("{norma_coordinate}.fab"));
    let source = fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    extract_public_modulus_functions(&source)
}

fn extract_public_modulus_functions(source: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut skip_next_function = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.contains("@ privata") {
            skip_next_function = true;
            continue;
        }
        let Some(name) = parse_function_name(trimmed) else {
            continue;
        };
        if skip_next_function || name.starts_with('_') {
            skip_next_function = false;
            continue;
        }
        names.insert(name.to_owned());
        skip_next_function = false;
    }
    names
}

fn parse_function_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("fn ")?;
    let head = rest.split('(').next()?.trim();
    let name = head.split_whitespace().next()?;
    Some(name.split('<').next().unwrap_or(name))
}

#[test]
fn kernel_prefix_detects_faber_imports() {
    assert!(is_kernel_import_path("faber:solum"));
    assert!(is_kernel_import_path("faber:json"));
    assert!(!is_kernel_import_path("norma:json"));
}

#[test]
fn kernel_manifest_verbs_are_subset_of_norma_public_surface() {
    for entry in kernel_manifest_entries() {
        let norma_functions = norma_module_public_functions(entry.module.name());
        for verb in entry.verbs {
            assert!(
                norma_functions.contains(*verb),
                "kernel manifest verb `{verb}` is missing from norma:{}.fab public surface",
                entry.module.name()
            );
        }
    }
}

#[test]
fn manifest_lists_exactly_six_v1_modules() {
    let entries = kernel_manifest_entries();
    assert_eq!(kernel_module_count(), 6);
    assert_eq!(entries.len(), 6);

    let names: Vec<_> = entries.iter().map(|entry| entry.module.name()).collect();
    assert_eq!(names, ["json", "solum", "processus", "aleator", "consolum", "toml"]);

    for entry in entries {
        assert_eq!(entry.import_path, entry.module.import_path());
        assert_eq!(entry.since, env!("CARGO_PKG_VERSION"));
        assert!(!entry.verbs.is_empty());
        assert!(!entry.provider_module.is_empty());
    }
}

#[test]
fn known_kernel_modules_resolve() {
    for entry in kernel_manifest_entries() {
        let resolved = resolve_kernel_module(entry.import_path).expect(entry.import_path);
        assert_eq!(resolved, entry.module);
    }
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
fn kernel_import_lowers_with_kernel_discriminator() {
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let source = r#"
importa ex "faber:processus" processus

incipit {
    nota processus.argv()
}
"#;
    let unit = analyze_source(&session, "kernel-import.fab", source, AnalyzeRequest::default()).expect("analysis");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("lower");

    let mut saw_kernel_processus = false;
    for function in &program.functions {
        for block in &function.blocks {
            for stmt in &block.statements {
                let MirStatementKind::RuntimeCall { call, .. } = &stmt.kind else {
                    continue;
                };
                let MirIntrinsic::Provider(provider) = &call.intrinsic else {
                    continue;
                };
                if provider.kind == MirProviderKind::Kernel(KernelModule::Processus) {
                    saw_kernel_processus = true;
                }
            }
        }
    }
    assert!(saw_kernel_processus, "expected kernel processus provider in lowered MIR");
}

#[test]
fn kernel_toml_import_lowers_with_kernel_discriminator() {
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let source = r#"
importa ex "faber:toml" toml

incipit {
    nota toml.parse("x = 1")
}
"#;
    let unit = analyze_source(&session, "kernel-toml-import.fab", source, AnalyzeRequest::default()).expect("analysis");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("lower");

    let mut saw_kernel_toml = false;
    for function in &program.functions {
        for block in &function.blocks {
            for stmt in &block.statements {
                let MirStatementKind::RuntimeCall { call, .. } = &stmt.kind else {
                    continue;
                };
                let MirIntrinsic::Provider(provider) = &call.intrinsic else {
                    continue;
                };
                if provider.kind == MirProviderKind::Kernel(KernelModule::Toml) {
                    saw_kernel_toml = true;
                }
            }
        }
    }
    assert!(saw_kernel_toml, "expected kernel toml provider in lowered MIR");
}

#[test]
fn kernel_glob_import_lowers_module_qualified_calls() {
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let source = r#"
importa ex "faber:*" faber

incipit {
    nota faber.json.stringify(faber.json.parse("[1]"))
    nota faber.aleator.uuid()
}
"#;
    let unit = analyze_source(&session, "kernel-glob-import.fab", source, AnalyzeRequest::default()).expect("analysis");
    let program = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("lower");

    let mut saw_json = false;
    let mut saw_aleator = false;
    for function in &program.functions {
        for block in &function.blocks {
            for stmt in &block.statements {
                let MirStatementKind::RuntimeCall { call, .. } = &stmt.kind else {
                    continue;
                };
                let MirIntrinsic::Provider(provider) = &call.intrinsic else {
                    continue;
                };
                saw_json |= provider.kind == MirProviderKind::Kernel(KernelModule::Json);
                saw_aleator |= provider.kind == MirProviderKind::Kernel(KernelModule::Aleator);
            }
        }
    }

    assert!(saw_json, "expected glob import to lower faber.json.* as kernel json provider");
    assert!(
        saw_aleator,
        "expected glob import to lower faber.aleator.* as kernel aleator provider"
    );
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
