//! Portable MIR package image (`FMIR`) format crate.
//!
//! Owns the wire/schema contract for the serialized MIR package image:
//! portable image structs, the text (`fmir-text`, TOML) and binary (`fmir`,
//! postcard) codecs, artifact-version and target-tag checks, and fail-closed
//! image validation.
//!
//! This is a format crate, not a package resolver, host, or runner:
//! package analysis and package-to-image assembly, package paths and target
//! layout, CLI planning, and
//! loading/launching policy stay in the sibling `faber` crate. Faber passes
//! its own toolchain version into [`decode_text_image`] / [`decode_binary_image`]
//! and maps [`FmirImageError`] to its diagnostics.

mod codec;
pub mod error;
pub mod schema;

pub use codec::{decode_binary_image, decode_text_image, encode_binary_image, encode_text_image};
pub use error::FmirImageError;
pub use schema::{
    ARTIFACT_ABI_VERSION, ARTIFACT_REFLECTION_VERSION, ArtifactCompatibilityRow,
    ArtifactCompatibilityTable, DeviceArtifactFormat, DevicePayloadEncoding,
    DevicePayloadEncodingError, DeviceTargetId, EXECUTOR_PLAN_SECTION_VERSION, FMIR_TARGET_NAME,
    FMIR_TEXT_TARGET_NAME, FmirArtifactReflection, FmirBinaryImageFile, FmirDeviceArtifact,
    FmirDeviceArtifactsSection, FmirDeviceBackend, FmirDeviceInput, FmirDeviceProgramSection,
    FmirDeviceSection, FmirDeviceSelection, FmirDeviceSymbol, FmirExecutorPlanSection,
    FmirReflectedKernel, FmirReflectedResource, FmirTextCliOperand, FmirTextCliRootSection,
    FmirTextCliSection, FmirTextCliValueType, FmirTextImageFile, FmirTextProgramSection,
    FmirTextRuntimeSection, FmirTextSourceIdentity, FmirTextSourcesSection,
    FmirTextToolchainSection, FmirTextTypesSection, MaterializationStage,
    PACKAGE_MIR_ARTIFACT_VERSION, WireBackendRequirement, WireBarrierPhase, WireBarrierPoint,
    WireBatchedMatMulPlan, WireBatchedRowSoftmaxPlan, WireBatchedTransposePlan, WireBindDescriptor,
    WireBinding, WireBufferIdentity, WireBufferLifetime, WireBufferRole, WireBufferVersion,
    WireCollectionKernelPlan, WireDependencyEdge, WireDeviceProgram, WireDeviceResource,
    WireDispatchSize, WireExecutorPlan, WireInitializationPolicy, WireKernelLaunchPlan,
    WireKernelUnit, WireLaunchUnit, WireLibraryBind, WireLogicalSignature, WireMatMulLayout,
    WireMatMulPlan, WireMatMulSharedMemory, WireObservationCadence, WireObservationFact,
    WireOffsetBinding, WireOobPaddingPolicy, WirePlanProvenance, WireProgramLifetime, WireReduceOp,
    WireReductionPlan, WireResourceAccess, WireResultBuffer, WireSemanticValue,
    WireSemanticValueOrigin, WireSharedMemoryLayout, WireStorageLayout, WireTransposePlan,
    WireUniformBinding, WireWorkgroupCount, WireWorkgroupSize, WireWorkspaceBinding,
    admit_device_section_with_compat, canonical_program_bytes, canonical_unpadded_base64,
    default_compatibility_table, device_identity_hash, is_known_host_requirement,
    is_known_target_feature, packet_sha256_canonical, sha256_digest, validate_artifact_combination,
    validate_compiled_in_target, validate_duplicate_target_features, validate_target_features,
};
pub use schema::{
    HOST_EXECUTION_SCHEMA, HOST_EXECUTION_WIRE_VERSION, HOST_INVOCATION_SCHEMA,
    HOST_INVOCATION_WIRE_VERSION, HostExecutionProjectionError, HostExecutionWireError,
    WireHostAccessV1, WireHostArtifactV1, WireHostBindingWindowV1, WireHostDependencyV1,
    WireHostExecutionEnvelopeV1, WireHostExecutionProgramV1, WireHostInitializationV1,
    WireHostInvocationEnvelopeV1, WireHostInvocationV1, WireHostInvocationWindowV1,
    WireHostKernelSlotV1, WireHostKernelV1, WireHostLaunchBindingV1, WireHostLaunchV1,
    WireHostObservationV1, WireHostResourceLifetimeV1, WireHostResourceV1, WireHostTargetV1,
    encode_host_execution_v1, encode_host_invocation_v1, host_execution_sha256,
    host_invocation_sha256, project_host_execution_v1,
};

#[cfg(test)]
#[path = "device_payload_test.rs"]
mod device_payload_test;

#[cfg(test)]
#[path = "inf_test.rs"]
mod inf_test;
