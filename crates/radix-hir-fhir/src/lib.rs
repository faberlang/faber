//! Portable analyzed-HIR artifact (`FHIR`) format crate.
//!
//! Owns the wire/schema contract for the serialized analyzed-HIR artifact:
//! portable schema types, the unit codec (`postcard` for unit artifacts, the
//! named-field `MessagePack` codec for the live `.fhirpkg` package envelope
//! since the FPF-5 switch), schema/version checks, and fail-closed
//! referential-integrity validation.
//!
//! This is a format crate, not a compiler driver: `AnalyzedModule`
//! reconstruction and driver orchestration stay in `radix-module`, which
//! calls [`decode`] and rebuilds arenas from the returned [`HirArtifact`].
//! Schema evolution and package/library requirements remain the authority of
//! the `hir-artifact-fhir` campaign.

mod validate;

pub mod artifact;
mod decode;
mod error;
mod msgpack;
pub mod package;
mod shape;

#[cfg(test)]
mod commune_emit_test;
#[cfg(test)]
mod insere_node_test;
#[cfg(test)]
mod ordo_order_test;
#[cfg(test)]
mod ranged_int_test;

#[cfg(test)]
mod ranged_list_test;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod widen_node_test;

pub use artifact::DisplayTokensWire;
pub use artifact::*;
pub use decode::{decode, encode};
pub use error::HirArtifactError;
pub use shape::wire_shape_hash;
// FPF-5: the MessagePack package codecs are the live `.fhirpkg` wire
// (package schema v2); the postcard package codec is the legacy v1 path.
pub use msgpack::{MsgpackError, decode_package_msgpack, encode_package_msgpack};
pub use package::{
    DecodedFhirPackage, DecodedFhirPackageModule, FhirPackage, FhirPackageError, FhirPackageModule,
    LEGACY_PACKAGE_SCHEMA_VERSION, LibraryImportWire, LocalLinkWire, PACKAGE_SCHEMA_VERSION,
    PackageDependencyWire, PackageIdentityWire, decode_package, encode_package,
};
