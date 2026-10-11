//! Target-neutral MIR device-program schema (A2 — the common device program).
//!
//! This module is the **shared-schema authority** for the differentiable-GPU
//! campaign (`docs/archived/gpu-training-lowering/`): the target-neutral
//! [`DeviceProgram`] representation that `hosts` sessions, the product CLI
//! (`faber`), and the FMIR device payload (S1-2) all consume. It is
//! single-writer in `radix` (campaign N2); nothing else in the workspace may
//! fork or embed a private copy of this schema.
//!
//! # Contract mapping (N1.6 / architecture record §6)
//!
//! The frozen contract (Stage 0 delivery spec §2 N1.6) names the
//! representation
//!
//! ```text
//! DeviceProgram { kernels: Vec<KernelUnit>, launches, lifetime, results }
//! ```
//!
//! and the ordered kernel unit (function id, entry, plan — `CollectionKernelPlan`
//! or elementwise — typed resource list, workgroup/dispatch/launch plan),
//! resources (buffer identity with role input/output/in-out, binding, element
//! type, element count, storage layout `DeviceHandle`/`HostOwned`), buffer
//! versions/lifetimes (per-program / per-step / observation point),
//! multi-output results (explicit result buffers), and shape-changing
//! operations (a new shape is a new version; no in-place reinterpretation).
//! The mapping is direct:
//!
//! | Contract concept | Schema type |
//! | --- | --- |
//! | `DeviceProgram` | [`DeviceProgram`] |
//! | ordered kernel unit | [`KernelUnit`] |
//! | function id | [`KernelUnit`]`.function` (`MirFunctionId`) |
//! | entry | [`KernelUnit`]`.entry` |
//! | plan (`CollectionKernelPlan` or elementwise) | [`KernelUnit`]`.plan` |
//! | typed resource list | [`KernelUnit`]`.resources` (`Vec<DeviceResource>`) |
//! | workgroup/dispatch/launch plan | [`KernelLaunchPlan`] |
//! | buffer identity | [`BufferIdentity`] (id, name, role, storage, lifetime) |
//! | binding / element type / element count | [`DeviceResource`] (binding + [`BufferVersion`] shape) |
//! | buffer versions / lifetimes | [`BufferVersion`] + [`BufferLifetime`] |
//! | multi-output results | [`ResultBuffer`] |
//! | shape-changing operations | version numbers (new shape ⇒ new version; see [`DeviceProgram::validate`]) |
//! | launches | [`LaunchUnit`] |
//! | lifetime | [`DeviceProgramLifetime`] |
//!
//! # WGSL chain-descriptor generalization (§6.3)
//!
//! The WGSL chain descriptor (`radix-mir-wgsl/src/chain_descriptor.rs`)
//! models an ordered kernel chain with per-kernel storage-buffer declarations,
//! bind-group layouts, output bindings, and positional inter-kernel identity
//! mappings. This schema generalizes that concept in common MIR without
//! promoting the WGSL schema verbatim:
//!
//! | WGSL chain descriptor | Generalized form |
//! | --- | --- |
//! | `chain: Vec<KernelDescriptor>` | ordered [`DeviceProgram`]`.kernels` + [`DeviceProgram`]`.launches` |
//! | `entry_point` + WGSL `source` | target-neutral [`KernelUnit`]`.entry`; no source text (A3) |
//! | `storage_buffers: Vec<StorageBufferDecl>` | typed [`DeviceResource`] list (element type + count + storage layout) |
//! | `bind_group_layout` / `BindGroupEntry` | [`DeviceResource`]`.binding` |
//! | `output_bindings` | explicit [`ResultBuffer`] entries |
//! | `InterKernelBufferIdentity` (output→input position pairs) | buffer identity + content version; producer/consumer edges derived in [`BufferRegistry`] and exposed as [`DataFlowPair`] |
//!
//! The WGSL descriptor is positional (kernel index + binding) because the WGSL
//! text is the identity. This schema keys data flow on a declared buffer
//! identity and a content version, so hosts never infer connectivity from
//! emitted text or binding positions (A3). What is **not** carried over: WGSL
//! source strings, per-kernel pipeline/bind-group reflection, and positional
//! identity — those are emitter/host concerns, not program facts.
//!
//! # Materialization seam (A2/A4) — S1-1 design-note decision
//!
//! **Decision**: the device program is produced by a **device-program
//! constructor over typed AIR/MIR facts** — never a structural clone of
//! `radix-air-bridge/src/to_mir.rs` and never a GPU-specific relowering.
//!
//! Evidence:
//! - `to_mir.rs` is a structural per-function lowering
//!   (`AirProgramToMir::lower_program` → `AirFunctionToMir::lower_function`)
//!   that reconstructs locals, statements, and CFG from AIR expressions. The
//!   device program needs none of that: it needs signatures, plans, typed
//!   resources, and launch plans, all of which already exist as typed facts on
//!   a validated MIR program.
//! - The typed facts the constructor composes are live today:
//!   [`MirKernelSignature`](crate::abi::MirKernelSignature) per device function
//!   (the `storage_buffer_kernel` family in `abi.rs`), the typed
//!   `MirCollectionOp → CollectionKernelPlan` mapping (council TB5,
//!   [`CollectionKernelPlan::from_contract`] in `kernel_plan.rs`, which already
//!   consumes [`CollectionOpContract`](crate::abi::CollectionOpContract),
//!   the typed op, and matmul shapes), and
//!   [`MirDeviceRole`](crate::device::MirDeviceRole) /
//!   [`MirDeviceContext`](crate::device::MirDeviceContext) (which functions are
//!   device-resident, and — via `classify_device_safe` keying on
//!   [`MirTensorStorageLayout::DeviceHandle`] /
//!   [`MirTensorLocusLayout::Device`] — which tensors are device-resident).
//! - A1/A4: `to_mir.rs` remains the single CPU materialization (untouched);
//!   typed collection calls remain valid compiler input; differentiation math
//!   stays in `radix-air`. A constructor that folds typed facts cannot fork
//!   VJP formulas or introduce GPU-specific AIR nodes.
//! - The building-block adapters in this module —
//!   [`KernelLaunchPlan::from_signature_and_function`],
//!   [`DeviceResource::from_kernel_resource`], [`BufferRole::from_abi_role`] —
//!   are exactly the typed-fact surface a materializer composes. The concrete
//!   materializer module (radix-air, beside `to_mir.rs`, or radix-mir beside
//!   `kernel_plan.rs`) is picked when the TB5 typed plan mapping lands (S1-3);
//!   S1-1 owns the schema and these adapters only.
//!
//! **Constructor shape**: `ValidatedMir` + `MirDeviceContext` + per-kernel
//! `MirKernelSignature` + `CollectionKernelPlan` → [`DeviceProgram`], via
//! [`DeviceProgram::new`] and the typed adapters above. If a later stage finds
//! this reading incompatible with the frozen schema, it stops and routes a
//! need to the Mind (delivery spec §11); it does not fork silently.

mod constructor;
mod error;
mod helpers;
mod host_wiring;
pub mod launch_geometry;
mod program;
mod registry;
mod types;

pub use constructor::*;
pub use error::*;
pub use registry::*;
pub use types::*;

#[cfg(test)]
#[path = "constructor_test.rs"]
mod constructor_tests;

#[cfg(test)]
#[path = "device_program_test.rs"]
mod tests;
