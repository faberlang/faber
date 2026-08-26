//! DFV2-1 entry-gating tests (`device-fragments-v2` delivery DFV2-1).
//!
//! The launch surface is explicit: `@ nucleum @ public` functions are the
//! only launchable entries (ABI, dispatch, reflection, inventory). A private
//! `@ nucleum` function is a module-internal helper — it produces no entry
//! row, while the composition path (`device_roles_from_hir`) still
//! classifies it as a device function.

use super::compile;
use crate::codegen::Target;
use crate::driver::{analyze_source, AnalyzeRequest, Config, ModuleCompileRequest, Session};
use crate::mir::{device_entry_roles_from_hir, device_roles_from_hir, MirDeviceRole};
use crate::test_support::test_session;

/// A public kernel and a private `@ nucleum` helper in one unit. Both are
/// device functions; only the public one is a launchable entry.
const PUBLIC_PRIVATE_PAIR: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn addita(tf32[256] a, tf32[256] b) → tf32[256] {
    return a.added(b)
}

@ kernel
fn hidden_helper(tf32[256] a, tf32[256] b) → tf32[256] {
    return a.added(b)
}
"#;

fn analyzed_pair() -> (crate::driver::AnalyzedModule, radix_hir::DefId, radix_hir::DefId) {
    let analysis = analyze_source(
        &test_session(Target::MirWgsl),
        "entry-pair.fab",
        PUBLIC_PRIVATE_PAIR,
        AnalyzeRequest::default(),
    )
    .unwrap_or_else(|diagnostics| panic!("analysis failed: {diagnostics:?}"));
    let public = def_id_for_function(&analysis, "addita");
    let private = def_id_for_function(&analysis, "hidden_helper");
    (analysis, public, private)
}

fn def_id_for_function(analysis: &crate::driver::AnalyzedModule, name: &str) -> radix_hir::DefId {
    analysis
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            radix_hir::HirItemKind::Function(func) if analysis.interner.resolve(func.name) == name => {
                Some(item.def_id)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no function named {name} in analyzed unit"))
}

#[test]
fn entry_roles_contain_only_the_public_kernel() {
    let (analysis, public, private) = analyzed_pair();
    let entries = device_entry_roles_from_hir(&analysis.hir, &analysis.interner);
    assert_eq!(
        entries.get(&public),
        Some(&MirDeviceRole::Kernel),
        "public kernel must be a launchable entry"
    );
    assert!(
        !entries.contains_key(&private),
        "private @ nucleum function must not produce an entry row"
    );
}

#[test]
fn composition_roles_keep_the_private_helper_classified() {
    let (analysis, public, private) = analyzed_pair();
    let roles = device_roles_from_hir(&analysis.hir);
    assert_eq!(roles.get(&public), Some(&MirDeviceRole::Kernel));
    assert_eq!(
        roles.get(&private),
        Some(&MirDeviceRole::Kernel),
        "private helpers stay device functions for the composition path"
    );
}

#[cfg(feature = "mir-llvm")]
#[test]
fn nvvm_emit_produces_exactly_one_entry_row_for_the_pair() {
    let session = Session::new(
        Config::default()
            .with_target(Target::MirLlvm)
            .with_dev_stdlib()
            .with_nvvm_device_module(),
    );
    let result = compile(
        &session,
        "entry-pair.fab",
        PUBLIC_PRIVATE_PAIR,
        ModuleCompileRequest::default(),
    );
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);
    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!("expected llvm-text output");
    };
    assert_eq!(
        output.code.matches("@nucleum device-entry").count(),
        1,
        "expected exactly one entry row (public kernel only):\n{}",
        output.code
    );
    assert!(
        !output.code.contains("ptr @hidden_helper, !\"kernel\""),
        "private helper must not appear in the NVVM kernel annotations:\n{}",
        output.code
    );
}
