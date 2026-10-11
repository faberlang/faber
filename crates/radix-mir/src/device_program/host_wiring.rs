//! Host call-site wiring facts for the device-program constructor (NAMEJOIN NJ1).
//!
//! WHY: buffer identity in a device program comes from VALUE identity — the
//! same host value passed as one kernel's output and the next kernel's input —
//! never from kernel parameter NAMES (operator decision 2026-10-08). The value
//! identity lives at the host call site: `Call { destination, callee, args }`
//! in a non-device function. This module is the pure collector of those facts;
//! it decides nothing about buffers.
//!
//! SLOT KEYS. A slot is `(kernel, key)`. `SlotKey::Param(i)` is the kernel's
//! `i`-th parameter, bound by call-site ARGUMENT POSITION (the same position
//! the run-time dispatcher binds host tensors by). `SlotKey::Result` is the
//! kernel's return-form output, bound by the call's `destination`.
//!
//! HOST VALUES. A host value is `(host function, local | temp)`. It is read
//! from an argument that is a bare place (no projections) or a temp operand,
//! and from a destination that is a bare place.
//!
//! UNKNOWN. A slot carries an unknown marker (so it can never join by value)
//! when any call site gives it no identity: a projected place, a constant, an
//! SSA value operand, or a discarded result. A kernel reached through anything
//! other than a direct host `Call` (closure, function value, `TryCall`) is
//! opaque: every one of its slots is unknown.
//!
//! DETERMINISM. Everything is keyed by ids in `BTreeMap` / `BTreeSet`, so
//! iteration order is stable and independent of interner symbols (renaming a
//! kernel parameter changes nothing collected here).

use std::collections::{BTreeMap, BTreeSet};

use crate::visit::{MirVisitor, walk_callee, walk_operand, walk_stmt, walk_value};
use crate::{
    MirCallee, MirConstant, MirFunction, MirFunctionId, MirLocalId, MirOperand, MirPlace,
    MirPlaceBase, MirProgram, MirStatement, MirStatementKind, MirTempId, MirValue, MirValueKind,
};
use radix_hir::DefId;

/// The storage a host value lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum HostBase {
    Local(MirLocalId),
    Temp(MirTempId),
}

/// One host value: a local or temp of one host function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct HostValue {
    pub(super) function: MirFunctionId,
    pub(super) base: HostBase,
}

/// Which of a kernel's bindings a call site feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum SlotKey {
    /// The kernel's parameter at this position (call-site argument position).
    Param(u32),
    /// The kernel's return-form output (the call's `destination`).
    Result,
}

/// A kernel slot: the unit value identity is collected for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct Slot {
    pub(super) kernel: MirFunctionId,
    pub(super) key: SlotKey,
}

impl Slot {
    #[must_use]
    pub(super) fn param(kernel: MirFunctionId, position: u32) -> Self {
        Self {
            kernel,
            key: SlotKey::Param(position),
        }
    }

    #[must_use]
    pub(super) fn result(kernel: MirFunctionId) -> Self {
        Self {
            kernel,
            key: SlotKey::Result,
        }
    }

    /// The parameter slot of `kernel` whose parameter local is `local`, when
    /// the kernel declares one (maps a constructor resource's `source_local`
    /// to its slot).
    #[must_use]
    pub(super) fn of_param_local(kernel: &MirFunction, local: MirLocalId) -> Option<Self> {
        let position = kernel
            .params
            .iter()
            .position(|param| param.local == local)?;
        Some(Self::param(kernel.id, u32::try_from(position).ok()?))
    }
}

/// Everything the host call sites say about one slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SlotSites {
    /// Every host value seen at a call site of the slot's kernel.
    pub(super) values: BTreeSet<HostValue>,
    /// Some call site gave the slot no identity.
    pub(super) unknown: bool,
}

/// Host call-site wiring of the selected kernels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct HostWiring {
    slots: BTreeMap<Slot, SlotSites>,
    /// Kernels reached through something other than a direct host `Call`.
    opaque: BTreeSet<MirFunctionId>,
}

impl HostWiring {
    /// The recorded sites of `slot`, when any call site touched it.
    #[cfg(test)]
    #[must_use]
    pub(super) fn sites(&self, slot: Slot) -> Option<&SlotSites> {
        self.slots.get(&slot)
    }

    /// Whether `kernel` was reached other than by a direct host `Call`.
    #[cfg(test)]
    #[must_use]
    pub(super) fn is_opaque(&self, kernel: MirFunctionId) -> bool {
        self.opaque.contains(&kernel)
    }

    /// Whether two slots hold the same host value(s): both sets non-empty,
    /// neither slot unknown (nor its kernel opaque), and the sets EQUAL.
    /// Partial overlap is not a join — a slot owning extra call sites owns
    /// its own buffer.
    #[must_use]
    pub(super) fn same_host_values(&self, a: Slot, b: Slot) -> bool {
        let (Some(a_sites), Some(b_sites)) = (self.known(a), self.known(b)) else {
            return false;
        };
        !a_sites.values.is_empty() && a_sites.values == b_sites.values
    }

    fn known(&self, slot: Slot) -> Option<&SlotSites> {
        if self.opaque.contains(&slot.kernel) {
            return None;
        }
        self.slots.get(&slot).filter(|sites| !sites.unknown)
    }

    fn record(&mut self, slot: Slot, value: Option<HostValue>) {
        let sites = self.slots.entry(slot).or_default();
        match value {
            Some(value) => {
                sites.values.insert(value);
            }
            None => sites.unknown = true,
        }
    }
}

/// Collect the host call-site wiring of `selected_kernels` from every host
/// (non-device-stage) function of `program`.
#[must_use]
pub(super) fn collect_host_wiring(
    program: &MirProgram,
    selected_kernels: &BTreeSet<MirFunctionId>,
) -> HostWiring {
    // Real lowering calls a named kernel through `MirCallee::Definition`
    // (the source `DefId`), not `MirCallee::Function`: resolve it to the
    // selected kernel whose `source` is that definition.
    let by_source = program
        .functions
        .iter()
        .filter(|function| selected_kernels.contains(&function.id))
        .filter_map(|function| Some((function.source?, function.id)))
        .collect();
    let mut collector = Collector {
        selected: selected_kernels,
        by_source,
        function: None,
        wiring: HostWiring::default(),
    };
    for function in program
        .functions
        .iter()
        .filter(|function| function.shader_stage.is_none())
    {
        collector.function = Some(function.id);
        collector.visit_function(function);
    }
    collector.wiring
}

struct Collector<'a> {
    selected: &'a BTreeSet<MirFunctionId>,
    /// Selected kernels by their source definition.
    by_source: Vec<(DefId, MirFunctionId)>,
    function: Option<MirFunctionId>,
    wiring: HostWiring,
}

impl Collector<'_> {
    /// The value identity of `base` inside the host function being visited
    /// (`None` outside any function).
    fn host_value(&self, base: &MirPlaceBase) -> Option<HostValue> {
        Some(HostValue {
            function: self.function?,
            base: match base {
                MirPlaceBase::Local(local) => HostBase::Local(*local),
                MirPlaceBase::Temp(temp) => HostBase::Temp(*temp),
            },
        })
    }

    fn place_identity(&self, place: &MirPlace) -> Option<HostValue> {
        if place.projections.is_empty() {
            self.host_value(&place.base)
        } else {
            None
        }
    }

    fn operand_identity(&self, operand: &MirOperand) -> Option<HostValue> {
        match operand {
            MirOperand::Place(place) => self.place_identity(place),
            MirOperand::Temp(temp) => self.host_value(&MirPlaceBase::Temp(*temp)),
            MirOperand::Value(_) | MirOperand::Constant(_) => None,
        }
    }

    fn selected_direct(&self, callee: &MirCallee) -> Option<MirFunctionId> {
        let id = match callee {
            MirCallee::Function(id)
            | MirCallee::Value(MirOperand::Constant(MirConstant::Function(id))) => *id,
            MirCallee::Definition { source, .. } => self.kernel_of_definition(*source)?,
            _ => return None,
        };
        self.selected.contains(&id).then_some(id)
    }

    fn kernel_of_definition(&self, source: DefId) -> Option<MirFunctionId> {
        self.by_source
            .iter()
            .find_map(|&(def, id)| (def == source).then_some(id))
    }

    fn mark_opaque(&mut self, id: MirFunctionId) {
        if self.selected.contains(&id) {
            self.wiring.opaque.insert(id);
        }
    }
}

impl MirVisitor for Collector<'_> {
    fn visit_stmt(&mut self, stmt: &MirStatement) {
        let MirStatementKind::Call {
            destination,
            callee,
            args,
        } = &stmt.kind
        else {
            walk_stmt(self, stmt);
            return;
        };
        let Some(kernel) = self.selected_direct(callee) else {
            walk_stmt(self, stmt);
            return;
        };
        for (position, arg) in args.iter().enumerate() {
            let slot = Slot::param(kernel, u32::try_from(position).unwrap_or(u32::MAX));
            let identity = self.operand_identity(arg);
            self.wiring.record(slot, identity);
            // A kernel passed as an argument value is reached indirectly.
            self.visit_operand(arg);
        }
        let identity = destination
            .as_ref()
            .and_then(|place| self.place_identity(place));
        self.wiring.record(Slot::result(kernel), identity);
    }

    /// Reached only for callees of calls that are not a direct host `Call` of
    /// a selected kernel (those return early in `visit_stmt`): a failable
    /// `TryCall`, a closure call, or a function value.
    fn visit_callee(&mut self, callee: &MirCallee) {
        match callee {
            MirCallee::Function(id)
            | MirCallee::Value(MirOperand::Constant(MirConstant::Function(id))) => {
                self.mark_opaque(*id);
            }
            MirCallee::Definition { source, .. } => {
                if let Some(id) = self.kernel_of_definition(*source) {
                    self.mark_opaque(id);
                }
            }
            MirCallee::Closure(closure) => self.mark_opaque(closure.function),
            MirCallee::Value(_) => {}
        }
        walk_callee(self, callee);
    }

    fn visit_value(&mut self, value: &MirValue) {
        if let MirValueKind::Closure(closure) = &value.kind {
            self.mark_opaque(closure.function);
        }
        walk_value(self, value);
    }

    fn visit_operand(&mut self, operand: &MirOperand) {
        if let MirOperand::Constant(MirConstant::Function(id)) = operand {
            self.mark_opaque(*id);
        }
        walk_operand(self, operand);
    }
}

#[cfg(test)]
#[path = "host_wiring_test.rs"]
mod tests;
