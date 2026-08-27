//! KPC-EMIT imported entry-role merge tests (`device-fragments-v2`).
//!
//! The merge is the target-role handoff: concrete imported registrations
//! from package lowering admit only public Kernel template sources as launch
//! entries; private helpers and Fragment roles stay composition-only; a fact
//! that cannot be proven against the merged program fails closed before any
//! emitter sees it.

use super::*;
use crate::{MirFunction, MirFunctionId, MirProgram};
use radix_hir::DefId;

fn template_function(id: MirFunctionId, source: DefId) -> MirFunction {
    MirFunction {
        id,
        source: Some(source),
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: Vec::new(),
        // The merge never inspects types; any semantic id constructs.
        return_ty: MirType::semantic(radix_types::TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn program_of(functions: &[MirFunction]) -> MirProgram {
    MirProgram { functions: functions.to_vec() }
}

fn kernel_fact(source: DefId, function: MirFunctionId) -> MirImportedEntryFact {
    MirImportedEntryFact {
        source,
        function,
        role: MirDeviceRole::Kernel,
        launchable: true,
    }
}

/// done_when (a)/(d): a launchable public Kernel registration supplies a
/// concrete entry — the zero-entry red is gone once the carrier has rows.
#[test]
fn launchable_public_kernel_registration_admits_the_template_source() {
    let source = DefId(2_000_000_100);
    let program = program_of(&[
        template_function(MirFunctionId(0), source),
        template_function(MirFunctionId(1), source),
    ]);
    // Two concrete instances of one public Kernel template (e.g. [2, 2] and
    // [8, 8]): one DefId row admits both through `attach_program`.
    let merged = merge_imported_entry_roles(
        FxHashMap::default(),
        &[
            kernel_fact(source, MirFunctionId(0)),
            kernel_fact(source, MirFunctionId(1)),
        ],
        &program,
    )
    .unwrap_or_else(|error| panic!("public kernel registration must admit: {error:?}"));
    assert_eq!(
        merged.get(&source),
        Some(&MirDeviceRole::Kernel),
        "imported public Kernel template source must be a launch entry"
    );
}

/// done_when (b): private imported helpers and Fragment-role registrations
/// produce no launch row — even mislabeled facts cannot buy one.
#[test]
fn private_helpers_and_fragments_never_admit_a_launch_row() {
    let helper_source = DefId(2_000_000_101);
    let fragment_source = DefId(2_000_000_102);
    let mislabeled_fragment = DefId(2_000_000_103);
    let program = program_of(&[
        template_function(MirFunctionId(0), helper_source),
        template_function(MirFunctionId(1), fragment_source),
        template_function(MirFunctionId(2), mislabeled_fragment),
    ]);
    let facts = [
        // Private helper: Kernel role, no launch admission.
        MirImportedEntryFact {
            source: helper_source,
            function: MirFunctionId(0),
            role: MirDeviceRole::Kernel,
            launchable: false,
        },
        // Fragment role: never a dispatch stage.
        MirImportedEntryFact {
            source: fragment_source,
            function: MirFunctionId(1),
            role: MirDeviceRole::Fragment,
            launchable: false,
        },
        // A mislabeled Fragment carrying launch admission still cannot enter.
        MirImportedEntryFact {
            source: mislabeled_fragment,
            function: MirFunctionId(2),
            role: MirDeviceRole::Fragment,
            launchable: true,
        },
    ];
    let merged = merge_imported_entry_roles(FxHashMap::default(), &facts, &program)
        .unwrap_or_else(|error| panic!("non-entry facts must merge clean: {error:?}"));
    assert!(
        merged.is_empty(),
        "private helper and Fragment facts must produce no launch row, got {merged:?}"
    );
}

/// done_when (c): a registration without a resolvable concrete instance
/// never reaches an emitter — the merge fails closed with the named issue.
#[test]
fn unresolved_instance_fails_closed_before_emission() {
    let source = DefId(2_000_000_104);
    // The program carries a different instance than the registration keys.
    let program = program_of(&[template_function(MirFunctionId(7), source)]);
    for missing in [
        // Function absent from the program entirely.
        kernel_fact(source, MirFunctionId(0)),
        // Function present in the program but keyed to another source.
        MirImportedEntryFact {
            source: DefId(2_000_000_105),
            function: MirFunctionId(7),
            role: MirDeviceRole::Kernel,
            launchable: true,
        },
    ] {
        let error = merge_imported_entry_roles(FxHashMap::default(), &[missing], &program)
            .expect_err("unresolved instance must fail closed");
        assert_eq!(
            error.issue(),
            IMPORTED_DEVICE_INSTANCE_UNRESOLVED,
            "unresolved instance must carry its stable issue identity"
        );
    }
}

/// The carrier must agree with the local declaration table: a local
/// non-Kernel role for the same source is a conflict, not an override.
#[test]
fn local_role_disagreement_fails_closed() {
    let source = DefId(2_000_000_106);
    let program = program_of(&[template_function(MirFunctionId(0), source)]);
    let local = FxHashMap::from_iter([(source, MirDeviceRole::Fragment)]);
    let error = merge_imported_entry_roles(local, &[kernel_fact(source, MirFunctionId(0))], &program)
        .expect_err("role disagreement must fail closed");
    assert_eq!(error.issue(), IMPORTED_DEVICE_ENTRY_ROLE_CONFLICT);
}

/// done_when (f): local entry behavior passes through unchanged — public
/// entries, private non-entries, and graphics Fragment roles all survive a
/// merge that adds imported rows.
#[test]
fn local_entry_behavior_is_unchanged_by_the_merge() {
    let local_public = DefId(1);
    let local_private = DefId(2);
    let local_fragment = DefId(3);
    let imported_source = DefId(2_000_000_107);
    let local = FxHashMap::from_iter([
        (local_public, MirDeviceRole::Kernel),
        (local_fragment, MirDeviceRole::Fragment),
    ]);
    let program = program_of(&[
        template_function(MirFunctionId(0), local_public),
        template_function(MirFunctionId(1), imported_source),
    ]);
    let merged = merge_imported_entry_roles(
        local,
        &[kernel_fact(imported_source, MirFunctionId(1))],
        &program,
    )
    .unwrap_or_else(|error| panic!("clean merge must succeed: {error:?}"));
    assert_eq!(merged.get(&local_public), Some(&MirDeviceRole::Kernel));
    assert_eq!(merged.get(&local_fragment), Some(&MirDeviceRole::Fragment));
    assert!(!merged.contains_key(&local_private));
    assert_eq!(merged.get(&imported_source), Some(&MirDeviceRole::Kernel));
    assert_eq!(merged.len(), 3, "one row per admitted source: {merged:?}");
}
