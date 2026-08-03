//! Portable MIR package image (`FMIR`) format crate.
//!
//! Owns the wire/schema contract for the serialized MIR package image:
//! portable image structs, the text (`fmir-text`, TOML) and binary (`fmir`,
//! postcard) codecs, artifact-version and target-tag checks, and fail-closed
//! image validation.
//!
//! This is a format crate, not a package resolver, stepper, host, or runner:
//! package analysis and package-to-image assembly, package paths and target
//! layout, CLI planning, generated `fmir-bin` runner construction, and
//! loading/launching policy stay in the sibling `faber` crate. Faber passes
//! its own toolchain version into [`decode_text_image`] / [`decode_binary_image`]
//! and maps [`FmirImageError`] to its diagnostics.

mod codec;
pub mod error;
pub mod schema;

pub use codec::{decode_binary_image, decode_text_image, encode_binary_image, encode_text_image};
pub use error::FmirImageError;
pub use schema::{
    device_identity_hash, fnv1a64, fnv1a64_blob_hash, is_known_device_requirement,
    is_known_host_requirement, is_known_requirement, FmirBinaryImageFile, FmirDeviceArtifact,
    FmirDeviceArtifactsSection, FmirDeviceBackend, FmirDeviceProgramSection, FmirDeviceSection,
    FmirDeviceSelection, FmirTextCliOperand,
    FmirTextCliRootSection, FmirTextCliSection, FmirTextCliValueType, FmirTextImageFile,
    FmirTextProgramSection, FmirTextRuntimeSection, FmirTextSourceIdentity, FmirTextSourcesSection,
    FmirTextToolchainSection, FmirTextTypesSection, FMIR_TARGET_NAME, FMIR_TEXT_TARGET_NAME,
    FNV1A64_OFFSET, FNV1A64_PRIME, PACKAGE_MIR_ARTIFACT_VERSION,
};

#[cfg(test)]
#[path = "device_payload_test.rs"]
mod device_payload_test;
