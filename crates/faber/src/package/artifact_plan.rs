//! Target-neutral package artifact planning (forwarded to `radix-package`).
//!
//! RP4: the artifact planner moved to `radix-package` with the emission
//! assembly; this module forwards the moved surface so faber package
//! consumers (composite plan, library link, target dispatch) keep their
//! existing import paths until RP7 removes the residue.

#[allow(unused_imports)] // identity/edge types consumed by package tests until RP7
pub use radix_package::artifact_plan::{
    plan_package, ArtifactKind, ArtifactNode, ArtifactPlan, BindingId, DependencyEdge,
    DependencyKind, ExportId, ModuleId, PackageId,
};
pub(crate) use radix_package::artifact_plan::{linked_library_crate_map, native_library_deps};
