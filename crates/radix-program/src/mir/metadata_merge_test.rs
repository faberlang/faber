use super::*;
use radix_mir::device::MirDeviceFunctionMetadata;
use radix_mir::{
    MirDeviceRole, MirFunctionOrigin, MirFunctionVisibility, MirKernelShaderStage,
    MirSyntheticFunctionIdentity,
};

fn synthetic_row(parent: u32, ordinal: u32) -> MirDeviceFunctionMetadata {
    MirDeviceFunctionMetadata {
        identity: MirSyntheticFunctionIdentity {
            parent: MirFunctionId(parent),
            ordinal,
        },
        role: MirDeviceRole::Kernel,
        shader_stage: MirKernelShaderStage::Compute,
        visibility: MirFunctionVisibility::Private,
        origin: MirFunctionOrigin::SyntheticKernelClosure,
        lane_domain_local: None,
        refused_backends: Default::default(),
        tier: radix_mir::device::KernelTier::Default,
    }
}

#[test]
fn metadata_rows_shift_keys_and_parents_with_the_program() {
    let mut incoming = MirDeviceFunctionMetadataMap::default();
    incoming.insert(MirFunctionId(1), synthetic_row(0, 2));
    let mut merged = MirDeviceFunctionMetadataMap::default();
    merge_shifted_device_function_metadata(&mut merged, &incoming, 10, Path::new("shift.fab"))
        .expect("shifted metadata merge");
    assert_eq!(merged.len(), 1);
    let row = merged.get(&MirFunctionId(11)).expect("shifted row key");
    assert_eq!(row.identity.parent, MirFunctionId(10));
    assert_eq!(row.identity.ordinal, 2);
    assert_eq!(row.origin, MirFunctionOrigin::SyntheticKernelClosure);
}

#[test]
fn zero_offset_metadata_merge_keeps_ids_verbatim() {
    let mut incoming = MirDeviceFunctionMetadataMap::default();
    incoming.insert(MirFunctionId(4), synthetic_row(3, 0));
    let mut merged = MirDeviceFunctionMetadataMap::default();
    merge_shifted_device_function_metadata(&mut merged, &incoming, 0, Path::new("same.fab"))
        .expect("verbatim metadata merge");
    assert!(merged.contains_key(&MirFunctionId(4)));
    assert_eq!(
        merged.get(&MirFunctionId(4)).expect("row").identity.parent,
        MirFunctionId(3)
    );
}

#[test]
fn overflowing_metadata_shift_fails_closed() {
    let mut incoming = MirDeviceFunctionMetadataMap::default();
    incoming.insert(MirFunctionId(u32::MAX), synthetic_row(0, 0));
    let mut merged = MirDeviceFunctionMetadataMap::default();
    let result =
        merge_shifted_device_function_metadata(&mut merged, &incoming, 1, Path::new("over.fab"));
    assert!(
        result.is_err(),
        "an unrepresentable shifted row never joins"
    );
    assert!(merged.is_empty(), "a failed merge inserts no partial rows");
}
