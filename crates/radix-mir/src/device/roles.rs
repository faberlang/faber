//! Shared MIR device roles and the per-function role context carried from HIR.

use crate::{MirFunction, MirFunctionId, MirProgram};
use radix_hir::{DefId, HirFunction, HirItemKind, HirModule};
use radix_lexer::Interner;
use rustc_hash::FxHashMap;

/// The `@ publica`/`@ public` annotation family: the launch-surface gate.
const PUBLICA_FAMILY: &str = "publica";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirDeviceRole {
    Kernel,
    /// `@ nucleum fragment` — composition role, not a dispatch stage.
    Fragment,
}

#[derive(Debug, Clone, Default)]
pub struct MirDeviceContext {
    pub(super) by_function_id: FxHashMap<MirFunctionId, MirDeviceRole>,
    pub(super) by_def_id: FxHashMap<DefId, MirDeviceRole>,
}

impl MirDeviceContext {
    #[must_use]
    pub fn from_hir_roles(roles_by_def: FxHashMap<DefId, MirDeviceRole>) -> Self {
        Self {
            by_def_id: roles_by_def,
            ..Self::default()
        }
    }

    pub fn attach_program(&mut self, program: &MirProgram) {
        for function in &program.functions {
            if let Some(def_id) = function.source {
                if let Some(role) = self.by_def_id.get(&def_id).copied() {
                    self.by_function_id.insert(function.id, role);
                }
            }
        }
    }

    #[must_use]
    pub fn role_for_function(&self, function: &MirFunction) -> Option<MirDeviceRole> {
        self.by_function_id
            .get(&function.id)
            .copied()
            .or_else(|| function.source.and_then(|def_id| self.role_for_def(def_id)))
    }

    #[must_use]
    pub fn role_for_def(&self, def_id: DefId) -> Option<MirDeviceRole> {
        self.by_def_id.get(&def_id).copied()
    }

    #[must_use]
    pub fn has_roles(&self) -> bool {
        !self.by_function_id.is_empty() || !self.by_def_id.is_empty()
    }
}

#[must_use]
pub fn device_roles_from_hir(hir: &HirModule) -> FxHashMap<DefId, MirDeviceRole> {
    let mut roles_by_def = FxHashMap::default();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) => {
                if let Some(role) = device_role_for_hir_function(func) {
                    roles_by_def.insert(item.def_id, role);
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if let Some(role) = device_role_for_hir_function(&method.func) {
                        roles_by_def.insert(method.def_id, role);
                    }
                }
            }
            _ => {}
        }
    }
    roles_by_def
}

#[must_use]
pub fn device_role_for_hir_function(func: &HirFunction) -> Option<MirDeviceRole> {
    // Fail-closed annotation exclusivity (CTO a7c616ed, extended NF-2):
    // Entry, Fragment, and graphics stages are mutually exclusive.
    let conflicting = [
        func.is_kernel,
        func.nucleum_fragment,
        func.is_vertex,
        func.is_fragment,
    ]
    .iter()
    .filter(|&&b| b)
    .count();
    if conflicting > 1 {
        return None;
    }
    if func.is_kernel {
        Some(MirDeviceRole::Kernel)
    } else if func.nucleum_fragment {
        Some(MirDeviceRole::Fragment)
    } else {
        None
    }
}

/// Whether a device function carries the launch-surface marker `@ publica`
/// (or the reader-locale spelling `@ public`).
///
/// This is the v2 visibility gate: `@ nucleum @ public` functions are
/// dispatchable entries; unmarked `@ nucleum` functions are module-internal
/// helpers that are never given a launch ABI.
#[must_use]
pub fn is_public_entry(func: &HirFunction, interner: &Interner) -> bool {
    func.annotations
        .iter()
        .any(|annotation| interner.resolve(annotation.family) == PUBLICA_FAMILY)
}

/// Launchability-aware device roles: only `@ nucleum @ public` functions are
/// classified as launchable `Kernel` entries. Private `@ nucleum` functions
/// stay helpers — they remain device functions for composition but produce no
/// entry row. `@ nucleum fragment` keeps the graphics `Fragment` role
/// unchanged (fragments are never launchable compute entries and the survivor
/// scan needs the role).
#[must_use]
pub fn device_entry_roles_from_hir(
    hir: &HirModule,
    interner: &Interner,
) -> FxHashMap<DefId, MirDeviceRole> {
    let mut roles_by_def = FxHashMap::default();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) => {
                if let Some(role) = device_entry_role_for_hir_function(func, interner) {
                    roles_by_def.insert(item.def_id, role);
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if let Some(role) = device_entry_role_for_hir_function(&method.func, interner) {
                        roles_by_def.insert(method.def_id, role);
                    }
                }
            }
            _ => {}
        }
    }
    roles_by_def
}

#[must_use]
fn device_entry_role_for_hir_function(
    func: &HirFunction,
    interner: &Interner,
) -> Option<MirDeviceRole> {
    match device_role_for_hir_function(func) {
        // The entry gate: a compute kernel is launchable only when public.
        Some(MirDeviceRole::Kernel) if is_public_entry(func, interner) => {
            Some(MirDeviceRole::Kernel)
        }
        // Fragments keep their role unchanged; they are never launchable
        // compute entries and the survivor scan still needs the classification.
        Some(MirDeviceRole::Fragment) => Some(MirDeviceRole::Fragment),
        // Private compute kernels are helpers: no launch role.
        Some(MirDeviceRole::Kernel) | None => None,
    }
}
