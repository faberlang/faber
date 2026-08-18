//! Shared MIR device roles and the per-function role context carried from HIR.

use crate::{MirFunction, MirFunctionId, MirProgram};
use radix_hir::{DefId, HirFunction, HirItemKind, HirModule};
use rustc_hash::FxHashMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirDeviceRole {
    Kernel,
    Device,
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
        self.by_function_id.get(&function.id).copied().or_else(|| {
            function
                .source
                .and_then(|def_id| self.by_def_id.get(&def_id).copied())
        })
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
    // Entry, Fragment, machina, and graphics stages are mutually exclusive.
    let conflicting = [
        func.is_kernel,
        func.nucleum_fragment,
        func.is_device,
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
    } else if func.is_device {
        Some(MirDeviceRole::Device)
    } else {
        None
    }
}
