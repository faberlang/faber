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

use super::loop_carry::{CallSite, call_order};
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
    /// `(a, b)`: some host function calls `a` before `b` within one iteration.
    before: BTreeSet<(MirFunctionId, MirFunctionId)>,
    /// `(a, b)`: some host function can run a call of `b` after a call of `a`.
    reaches: BTreeSet<(MirFunctionId, MirFunctionId)>,
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

    /// Whether a kernel call's RETURN value is re-fed to a kernel parameter:
    /// `result` is a result slot, and some call — of any kernel, including
    /// the producer's own next iteration (a host loop's carry) — passes one
    /// of the values that slot's calls produced into a parameter slot. A
    /// recorded host value is a fact, so a PARTIAL overlap already proves the
    /// consumption the strict buffer joins cannot express: a repeated
    /// producer's value set strictly contains a chained consumer's, and a
    /// kernel's own slots never value-join. Parameter and result slots of
    /// one call can never share a value (a value is produced before it is
    /// fed), so any shared value is a later call's read.
    #[must_use]
    pub(super) fn result_value_feeds_a_param(&self, result: Slot) -> bool {
        if !matches!(result.key, SlotKey::Result) {
            return false;
        }
        let Some(sites) = self.known(result) else {
            return false;
        };
        !sites.values.is_empty()
            && self.slots.iter().any(|(slot, slot_sites)| {
                matches!(slot.key, SlotKey::Param(_))
                    && slot_sites
                        .values
                        .iter()
                        .any(|value| sites.values.contains(value))
            })
    }

    /// Whether data `producer` writes reaches `consumer` only through a host
    /// loop's carry: the host can run a call of `consumer` after a call of
    /// `producer`, and never calls `producer` ahead of `consumer` within one
    /// iteration. A kernel reached other than by a direct host `Call` has no
    /// known call order, so no pair naming it is loop-carried.
    #[must_use]
    pub(super) fn is_loop_carried(&self, producer: MirFunctionId, consumer: MirFunctionId) -> bool {
        producer != consumer
            && !self.opaque.contains(&producer)
            && !self.opaque.contains(&consumer)
            && self.reaches.contains(&(producer, consumer))
            && !self.before.contains(&(producer, consumer))
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
        .filter_map(|function| function.source.map(|source| (source, function.id)))
        .collect();
    let mut collector = Collector {
        selected: selected_kernels,
        by_source,
        function: None,
        temp_homes: BTreeMap::new(),
        wiring: HostWiring::default(),
    };
    for function in program
        .functions
        .iter()
        .filter(|function| function.shader_stage.is_none())
    {
        collector.function = Some(function.id);
        collector.temp_homes = temp_homes(function);
        collector.visit_function(function);
        let sites = collector.call_sites(function);
        let order = call_order(function, &sites);
        collector.wiring.before.extend(order.before);
        collector.wiring.reaches.extend(order.reaches);
    }
    collector.wiring
}

struct Collector<'a> {
    selected: &'a BTreeSet<MirFunctionId>,
    /// Selected kernels by their source definition.
    by_source: Vec<(DefId, MirFunctionId)>,
    function: Option<MirFunctionId>,
    /// The local each temp of the function being visited is moved into by a
    /// plain operand assign (`Temp(k)` is the call destination, the local is
    /// the `let` binding). A temp moved into more than one local has no
    /// home. Canonicalizing the temp to its local's identity is what joins a
    /// call's result slot to the later call sites that read the binding —
    /// without it the lowering's temp-then-move shape would record the
    /// producer and the consumer under different identities.
    temp_homes: BTreeMap<MirTempId, MirLocalId>,
    wiring: HostWiring,
}

/// The local each temp lands in, from the function's plain operand assigns:
/// `local ← Temp(k)` (the lowering's temp-then-move shape for a call whose
/// result binds to a `let`). A temp assigned into two different locals is
/// ambiguous and gets no home; a local reassigned from several temps is the
/// loop-carry shape and keeps every temp's home.
fn temp_homes(function: &MirFunction) -> BTreeMap<MirTempId, MirLocalId> {
    let mut homes: BTreeMap<MirTempId, MirLocalId> = BTreeMap::new();
    let mut ambiguous: BTreeSet<MirTempId> = BTreeSet::new();
    let moved_temp = |value: &MirValue| -> Option<MirTempId> {
        let MirValueKind::Operand(operand) = &value.kind else {
            return None;
        };
        match operand {
            MirOperand::Temp(temp) => Some(*temp),
            MirOperand::Place(place) if place.projections.is_empty() => match place.base {
                MirPlaceBase::Temp(temp) => Some(temp),
                MirPlaceBase::Local(_) => None,
            },
            _ => None,
        }
    };
    for data in &function.blocks {
        for statement in &data.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let Some(temp) = moved_temp(value) else {
                continue;
            };
            if place.projections.is_empty()
                && let MirPlaceBase::Local(local) = place.base
                && homes.insert(temp, local).is_some_and(|home| home != local)
            {
                ambiguous.insert(temp);
            }
        }
    }
    homes.retain(|temp, _| !ambiguous.contains(temp));
    homes
}

impl Collector<'_> {
    /// The value identity of `base` inside the host function being visited
    /// (`None` outside any function). A temp with a home carries its local's
    /// identity: the call result it holds is the binding the later call
    /// sites read.
    fn host_value(&self, base: &MirPlaceBase) -> Option<HostValue> {
        let base = match base {
            MirPlaceBase::Local(local) => HostBase::Local(*local),
            MirPlaceBase::Temp(temp) => self
                .temp_homes
                .get(temp)
                .map_or(HostBase::Temp(*temp), |local| HostBase::Local(*local)),
        };
        Some(HostValue {
            function: self.function?,
            base,
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

    /// The direct call sites of selected kernels in `function`, by block and
    /// statement position.
    fn call_sites(&self, function: &MirFunction) -> Vec<CallSite> {
        let mut sites = Vec::new();
        for (block, data) in function.blocks.iter().enumerate() {
            for (statement, stmt) in data.statements.iter().enumerate() {
                let MirStatementKind::Call { callee, .. } = &stmt.kind else {
                    continue;
                };
                if let Some(kernel) = self.selected_direct(callee) {
                    sites.push(CallSite {
                        kernel,
                        block,
                        statement,
                    });
                }
            }
        }
        sites
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
