//! Portable analyzed-HIR artifact (`FHIR`) format crate.
//!
//! Owns the wire/schema contract for the serialized analyzed-HIR artifact:
//! portable schema types, the deterministic `postcard` codec, schema/version
//! checks, and fail-closed referential-integrity validation.
//!
//! This is a format crate, not a compiler driver: `AnalyzedUnit`
//! reconstruction and driver orchestration stay in the `radix` package, which
//! calls [`decode`] and rebuilds arenas from the returned [`HirArtifact`].
//! Schema evolution and package/library requirements remain the authority of
//! the `hir-artifact-fhir` campaign.

mod validate;

pub mod artifact;
mod decode;
mod error;
pub mod package;

#[cfg(test)]
mod test_support;

pub use artifact::*;
pub use decode::{decode, encode};
pub use error::HirArtifactError;
pub use package::{
    decode_package, encode_package, DecodedFhirPackage, DecodedFhirPackageModule, FhirPackage,
    FhirPackageError, FhirPackageModule, LibraryImportWire, LocalLinkWire, PackageDependencyWire,
    PackageIdentityWire, PACKAGE_SCHEMA_VERSION,
};
