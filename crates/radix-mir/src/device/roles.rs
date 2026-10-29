//! Shared MIR device roles and the per-function role context carried from HIR.

use crate::{
    MirFunction, MirFunctionId, MirFunctionOrigin, MirFunctionVisibility, MirKernelShaderStage,
    MirLocalId, MirProgram, MirSyntheticFunctionIdentity,
};
use handroll::FxHashMap;
pub use radix_hir::KernelTier;
use radix_hir::{DefId, HirFunction, HirItemKind, HirModule};
use radix_lexer::Interner;

/// Explicit device facts for a source-derived or source-less function.
///
/// Consumers must use this carrier instead of inferring private status from a
/// missing source definition, a generated name, or the compute stage alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirDeviceFunctionMetadata {
    pub identity: MirSyntheticFunctionIdentity,
    pub role: MirDeviceRole,
    pub shader_stage: MirKernelShaderStage,
    pub visibility: MirFunctionVisibility,
    pub origin: MirFunctionOrigin,
    /// The exact tensor local used as the source of a top-level lane-form
    /// region. `None` for functions without a lane-form source or when the
    /// source is not a local place that can resolve to an ABI resource. A
    /// shift view (`src.shift(s).limes<P>()`) carries no storage of its own,
    /// so a region over one records the view's base parameter.
    pub lane_domain_local: Option<MirLocalId>,
    /// The portable device backends whose inferred per-target requirement row
    /// refuses this function's kernel body (unit-local semantic facts carried
    /// to the emit boundary). Empty for a function the rows admit everywhere
    /// and for a source-less function whose parent is not a kernel.
    pub refused_backends: MirRefusedBackends,
    /// The kernel's effective safety tier (ruling 2026-10-28): what the
    /// author declared, lowered to [`KernelTier::Default`] by the safe switch.
    /// Device emitters read this to choose their math and index-check forms.
    /// Source-less synthetic kernels (closures, scan steps, region
    /// lowerings) carry [`KernelTier::Default`].
    pub tier: KernelTier,
}

/// A portable device backend that has a per-target kernel requirement row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirDeviceBackend {
    Amd,
    Metal,
    Wgsl,
}

impl MirDeviceBackend {
    const fn bit(self) -> u8 {
        match self {
            Self::Amd => 1,
            Self::Metal => 2,
            Self::Wgsl => 4,
        }
    }
}

/// A set of [`MirDeviceBackend`]s that refuse a kernel by name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MirRefusedBackends(u8);

impl MirRefusedBackends {
    /// This set with `backend` added.
    #[must_use]
    pub const fn with(self, backend: MirDeviceBackend) -> Self {
        Self(self.0 | backend.bit())
    }

    /// Whether `backend` refuses the function.
    #[must_use]
    pub const fn refuses(self, backend: MirDeviceBackend) -> bool {
        self.0 & backend.bit() != 0
    }
}

/// The functions of `metadata` that `backend` refuses, in function-id order.
#[must_use]
pub fn functions_refused_by(
    metadata: &MirDeviceFunctionMetadataMap,
    backend: MirDeviceBackend,
) -> Vec<MirFunctionId> {
    let mut refused: Vec<MirFunctionId> = metadata
        .iter()
        .filter(|(_, row)| row.refused_backends.refuses(backend))
        .map(|(id, _)| *id)
        .collect();
    refused.sort();
    refused
}

impl MirDeviceFunctionMetadata {
    /// The explicit launchability fact shared by HIR entry-role selection and
    /// MIR consumers: only a public `Kernel` role receives a launch row.
    #[must_use]
    pub fn entry_role(&self) -> Option<MirDeviceRole> {
        device_entry_role_for_visibility(
            self.role,
            self.visibility == MirFunctionVisibility::Public,
        )
    }
}

pub type MirDeviceFunctionMetadataMap = FxHashMap<MirFunctionId, MirDeviceFunctionMetadata>;

/// The `@ publica`/`@ public` annotation family: the launch-surface gate.
const PUBLICA_FAMILY: &str = "publica";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirDeviceRole {
    Kernel,
    /// `@ nucleum fragment` — composition role, not a dispatch stage.
    Fragment,
}

// `by_*` fields key different id spaces (function id, def id, function
// metadata) into the same role context; the prefix is the convention.
#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, Default)]
pub struct MirDeviceContext {
    pub(super) by_function_id: FxHashMap<MirFunctionId, MirDeviceRole>,
    pub(super) by_def_id: FxHashMap<DefId, MirDeviceRole>,
    pub(super) by_function_metadata: MirDeviceFunctionMetadataMap,
}

impl MirDeviceContext {
    #[must_use]
    pub fn from_hir_roles(roles_by_def: FxHashMap<DefId, MirDeviceRole>) -> Self {
        Self {
            by_def_id: roles_by_def,
            ..Self::default()
        }
    }

    /// Build a role context with explicit facts for source-derived and
    /// source-less functions.
    #[must_use]
    pub fn from_hir_roles_with_metadata(
        roles_by_def: FxHashMap<DefId, MirDeviceRole>,
        metadata: &MirDeviceFunctionMetadataMap,
    ) -> Self {
        let mut context = Self::from_hir_roles(roles_by_def);
        context.by_function_metadata.clone_from(metadata);
        context
    }

    pub fn attach_program(&mut self, program: &MirProgram) {
        for function in &program.functions {
            if let Some(def_id) = function.source
                && let Some(role) = self.by_def_id.get(&def_id).copied()
            {
                self.by_function_id.insert(function.id, role);
            }
            if let Some(metadata) = self.by_function_metadata.get(&function.id).copied() {
                self.by_function_id.insert(function.id, metadata.role);
            }
        }
    }

    /// Attach explicit function facts after source-based roles.
    ///
    /// A metadata row is accepted only for a function present in the program;
    /// an absent row cannot silently create a device role for a missing body.
    pub fn attach_program_with_metadata(
        &mut self,
        program: &MirProgram,
        metadata: &MirDeviceFunctionMetadataMap,
    ) {
        for function in &program.functions {
            if let Some(facts) = metadata.get(&function.id).copied() {
                self.by_function_metadata.insert(function.id, facts);
            }
        }
        self.attach_program(program);
    }

    #[must_use]
    pub fn role_for_function(&self, function: &MirFunction) -> Option<MirDeviceRole> {
        self.by_function_id
            .get(&function.id)
            .copied()
            .or_else(|| {
                self.by_function_metadata
                    .get(&function.id)
                    .map(|metadata| metadata.role)
            })
            .or_else(|| function.source.and_then(|def_id| self.role_for_def(def_id)))
    }

    #[must_use]
    pub fn metadata_for_function(
        &self,
        function: &MirFunction,
    ) -> Option<MirDeviceFunctionMetadata> {
        self.by_function_metadata.get(&function.id).copied()
    }

    #[must_use]
    pub fn role_for_def(&self, def_id: DefId) -> Option<MirDeviceRole> {
        self.by_def_id.get(&def_id).copied()
    }

    /// The declared per-def role map, for callers that hand roles to an
    /// emitter probe (the probes take the map, not a built context).
    #[must_use]
    pub fn roles_by_def(&self) -> &FxHashMap<DefId, MirDeviceRole> {
        &self.by_def_id
    }

    /// This context with no role for any of `functions`: their explicit
    /// function facts are dropped, and so is the declared role of their
    /// source definitions. A backend whose own emitter is the fail-closed
    /// authority for those functions (a lane emitter) gates the rest of the
    /// program through the shared classifier with this context.
    #[must_use]
    pub fn without_functions(&self, functions: &[&MirFunction]) -> Self {
        let mut context = self.clone();
        for function in functions {
            context.by_function_id.remove(&function.id);
            context.by_function_metadata.remove(&function.id);
            if let Some(def_id) = function.source {
                context.by_def_id.remove(&def_id);
            }
        }
        context
    }

    #[must_use]
    pub fn has_roles(&self) -> bool {
        !self.by_function_id.is_empty()
            || !self.by_def_id.is_empty()
            || !self.by_function_metadata.is_empty()
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
pub fn is_public_entry(annotations: &[radix_hir::HirAnnotation], interner: &Interner) -> bool {
    annotations
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
                if let Some(role) =
                    device_entry_role_for_hir_function(func, &item.annotations, interner)
                {
                    roles_by_def.insert(item.def_id, role);
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if let Some(role) = device_entry_role_for_hir_function(
                        &method.func,
                        &method.annotations,
                        interner,
                    ) {
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
fn device_entry_role_for_visibility(role: MirDeviceRole, public: bool) -> Option<MirDeviceRole> {
    match (role, public) {
        // The entry gate: a compute kernel is launchable only when public.
        (MirDeviceRole::Kernel, true) => Some(MirDeviceRole::Kernel),
        // Fragments keep their role unchanged; they are never launchable
        // compute entries and the survivor scan still needs the role.
        (MirDeviceRole::Fragment, _) => Some(MirDeviceRole::Fragment),
        // Private compute kernels are helpers: no launch role.
        (MirDeviceRole::Kernel, false) => None,
    }
}

#[must_use]
fn device_entry_role_for_hir_function(
    func: &HirFunction,
    annotations: &[radix_hir::HirAnnotation],
    interner: &Interner,
) -> Option<MirDeviceRole> {
    device_role_for_hir_function(func).and_then(|role| {
        device_entry_role_for_visibility(role, is_public_entry(annotations, interner))
    })
}

/// One concrete imported device instance's target-admission facts (KPC-EMIT).
///
/// Built from package lowering's `ImportedDeviceRegistration` rows and the
/// carried `InterfaceDeviceContract` — never re-derived from HIR, function
/// names, emitted text, or a second ambient cache (KPC metadata-carrier
/// decision). The builder decides launchability from the contract alone:
/// a public, composable Kernel route; everything else stays composable
/// without a launch row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirImportedEntryFact {
    /// Merged synthetic template source the instance was instantiated from
    /// (`MirMonomorphizationKey::source`).
    pub source: DefId,
    /// Concrete instance function in the merged program.
    pub function: MirFunctionId,
    /// v2 device role carried by the route's contract.
    pub role: MirDeviceRole,
    /// Contract launch admission: public Kernel route with a usable body.
    pub launchable: bool,
}

/// Fail-closed merge outcomes for [`merge_imported_entry_roles`] (KPC-EMIT
/// `done_when` (c)/(e)): a fact that cannot be proven against the merged
/// program never reaches an emitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirImportedEntryError {
    /// The registration's concrete instance is absent from the merged
    /// program (or keyed to a different template source) — an unresolved
    /// instance, never a silent admission.
    InstanceUnresolved {
        source: DefId,
        function: MirFunctionId,
    },
    /// The local declaration table and the imported contract disagree on
    /// the role for one source; the carrier is inconsistent, not ambiguous.
    RoleConflict {
        source: DefId,
        local: MirDeviceRole,
        imported: MirDeviceRole,
    },
}

/// Stable machine-readable identities for [`MirImportedEntryError`] (the
/// driver `issue` diagnostic arg), matching the `imported_device_*` family
/// (`imported_device_body_missing`, KPC-WIRE).
pub const IMPORTED_DEVICE_INSTANCE_UNRESOLVED: &str = "imported_device_instance_unresolved";
/// Stable identity for a local/imported role disagreement on one source.
pub const IMPORTED_DEVICE_ENTRY_ROLE_CONFLICT: &str = "imported_device_entry_role_conflict";

impl MirImportedEntryError {
    /// Stable machine-readable identity for the driver's `issue` arg.
    #[must_use]
    pub fn issue(&self) -> &'static str {
        match self {
            Self::InstanceUnresolved { .. } => IMPORTED_DEVICE_INSTANCE_UNRESOLVED,
            Self::RoleConflict { .. } => IMPORTED_DEVICE_ENTRY_ROLE_CONFLICT,
        }
    }

    /// Human-facing message naming the failed fact.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::InstanceUnresolved { source, function } => format!(
                "imported device registration f{} (def#{}) has no concrete instance in the merged program",
                function.0, source.0
            ),
            Self::RoleConflict {
                source,
                local,
                imported,
            } => format!(
                "imported device registration def#{} conflicts with the local {:?} role (imported {:?})",
                source.0, local, imported
            ),
        }
    }
}

/// Merge imported concrete entry facts into the local entry-role map
/// (KPC-EMIT target-role handoff).
///
/// Only launchable Kernel facts admit their template source as a `Kernel`
/// entry — one `DefId` per route, so every concrete instance of a public
/// Kernel template attaches through `MirDeviceContext::attach_program`.
/// Private imported helpers and Fragment-role registrations produce no
/// launch row (composition availability is decided earlier, from the full
/// role map, and is unchanged here). Local entries — public, private, and
/// graphics fragments — pass through untouched.
///
/// # Errors
///
/// Fails closed with [`MirImportedEntryError`] when a launchable fact's
/// concrete instance is not present in `program` (unresolved instance) or a
/// local role already disagrees for the same source.
// FxHashMap parameters are intentional (mirrors `MirDeviceContext` /
// interner maps); do not generalize over `BuildHasher` for call-site
// simplicity.
#[allow(clippy::implicit_hasher)]
pub fn merge_imported_entry_roles(
    mut roles_by_def: FxHashMap<DefId, MirDeviceRole>,
    facts: &[MirImportedEntryFact],
    program: &MirProgram,
) -> Result<FxHashMap<DefId, MirDeviceRole>, MirImportedEntryError> {
    for fact in facts {
        // The launch gate is double-checked here (role + contract admission):
        // a Fragment or private helper fact registers no launch row even if
        // a caller mislabels one as launchable.
        if !fact.launchable || fact.role != MirDeviceRole::Kernel {
            continue;
        }
        if !program
            .functions
            .iter()
            .any(|function| function.id == fact.function && function.source == Some(fact.source))
        {
            return Err(MirImportedEntryError::InstanceUnresolved {
                source: fact.source,
                function: fact.function,
            });
        }
        if let Some(local) = roles_by_def.get(&fact.source)
            && *local != MirDeviceRole::Kernel
        {
            return Err(MirImportedEntryError::RoleConflict {
                source: fact.source,
                local: *local,
                imported: fact.role,
            });
        }
        roles_by_def.insert(fact.source, MirDeviceRole::Kernel);
    }
    Ok(roles_by_def)
}
