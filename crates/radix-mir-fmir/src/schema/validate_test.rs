use super::super::wire::{
    WireAxisReductionPlan, WireBinding, WireBufferIdentity, WireBufferLifetime, WireBufferRole,
    WireBufferVersion, WireCollectionKernelPlan, WireDeviceProgram, WireDeviceResource,
    WireDispatchSize, WireInitializationPolicy, WireKernelLaunchPlan, WireKernelUnit,
    WireLaunchUnit, WireObservationCadence, WireObservationFact, WireProgramLifetime, WireReduceOp,
    WireReducedProjection, WireResourceAccess, WireResultBuffer, WireSemanticValue,
    WireSemanticValueOrigin, WireStorageLayout, WireWorkgroupCount, WireWorkgroupSize,
};
use super::*;

/// One-kernel axis-reduction program (`[8, 8]` → 8 elements along axis
/// 0, `Sum`). The reduced output version and the plan carry the same
/// projection; F1–F7 facts are otherwise well-formed so the fixture
/// isolates the projection under test.
fn reduced_projection_fixture() -> WireDeviceProgram {
    let w = WireBufferIdentity {
        id: 1,
        name: "w".to_owned(),
        role: WireBufferRole::Input,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::PerProgram,
        semantic_value: 1,
    };
    let o = WireBufferIdentity {
        id: 2,
        name: "o".to_owned(),
        role: WireBufferRole::Output,
        storage: WireStorageLayout::DeviceHandle,
        lifetime: WireBufferLifetime::ObservationPoint,
        semantic_value: 2,
    };
    let v_w = WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: 64,
        reduced_projection: None,
    };
    let v_o = WireBufferVersion {
        version: 1,
        element_ty: "f32".to_owned(),
        element_count: 8,
        reduced_projection: Some(WireReducedProjection {
            axis_extent: 8,
            inner_stride: 8,
        }),
    };
    let launch = WireKernelLaunchPlan {
        workgroup: WireWorkgroupSize { x: 8, y: 1, z: 1 },
        dispatch_size: WireDispatchSize { x: 1, y: 1, z: 1 },
        workgroup_count: WireWorkgroupCount { x: 1, y: 1, z: 1 },
    };
    WireDeviceProgram {
        kernels: vec![WireKernelUnit {
            function: 0,
            entry: "reduce".to_owned(),
            plan: WireCollectionKernelPlan::AxisReduction(WireAxisReductionPlan {
                op: WireReduceOp::Sum,
                axis: 0,
                projection: WireReducedProjection {
                    axis_extent: 8,
                    inner_stride: 8,
                },
            }),
            resources: vec![
                WireDeviceResource {
                    buffer: w.clone(),
                    version: v_w.clone(),
                    binding: WireBinding {
                        group: 0,
                        binding: 0,
                    },
                    access: WireResourceAccess::Read,
                    generation: 1,
                    initialization: WireInitializationPolicy::HostProvided,
                },
                WireDeviceResource {
                    buffer: o.clone(),
                    version: v_o.clone(),
                    binding: WireBinding {
                        group: 0,
                        binding: 1,
                    },
                    access: WireResourceAccess::Write,
                    generation: 1,
                    initialization: WireInitializationPolicy::KernelInitialized,
                },
            ],
            launch,
        }],
        launches: vec![WireLaunchUnit {
            id: 1,
            kernel_index: 0,
        }],
        lifetime: WireProgramLifetime::SingleRun,
        results: vec![WireResultBuffer {
            buffer: o.clone(),
            version: v_o.clone(),
            role: WireBufferRole::Output,
            produced_by: 1,
            observation: WireObservationFact {
                at_launch: 1,
                cadence: WireObservationCadence::PerStep,
            },
        }],
        semantic_values: vec![
            WireSemanticValue {
                id: 1,
                name: "w".to_owned(),
                origin: WireSemanticValueOrigin::HostInput,
            },
            WireSemanticValue {
                id: 2,
                name: "o".to_owned(),
                origin: WireSemanticValueOrigin::Synthetic {
                    label: "reduce-output".to_owned(),
                },
            },
        ],
        roots: vec![1],
        dependencies: Vec::new(),
        relations: Vec::new(),
    }
}

/// A consistent reduced-resource projection admits at the decode boundary.
#[test]
fn valid_reduced_projection_admits() {
    validate_wire_program(&reduced_projection_fixture())
        .expect("a consistent reduced-resource projection admits fail-closed");
}

/// A zero plan `axis_extent` and a zero buffer `inner_stride` both
/// reject with the positivity diagnostic.
#[test]
fn zero_reduced_projection_extent_rejected() {
    let mut wire = reduced_projection_fixture();
    let WireCollectionKernelPlan::AxisReduction(plan) = &mut wire.kernels[0].plan else {
        panic!("fixture carries an axis-reduction plan");
    };
    plan.projection.axis_extent = 0;
    let error = validate_wire_program(&wire)
        .expect_err("a zero axis extent fails the projection admission closed");
    assert!(
        error.to_string().contains("non-positive"),
        "the fail-closed diagnostic names the positivity rule: {error}"
    );

    let mut wire = reduced_projection_fixture();
    wire.kernels[0].resources[1].version.reduced_projection = Some(WireReducedProjection {
        axis_extent: 8,
        inner_stride: 0,
    });
    let error = validate_wire_program(&wire)
        .expect_err("a zero inner stride fails the projection admission closed");
    assert!(
        error.to_string().contains("non-positive"),
        "the fail-closed diagnostic names the positivity rule: {error}"
    );
}

/// The plan projection and the reduced output version must stamp the
/// same producer fact.
#[test]
fn mismatched_plan_vs_buffer_projection_rejected() {
    let mut wire = reduced_projection_fixture();
    let WireCollectionKernelPlan::AxisReduction(plan) = &mut wire.kernels[0].plan else {
        panic!("fixture carries an axis-reduction plan");
    };
    plan.projection.inner_stride = 4;
    let error = validate_wire_program(&wire)
        .expect_err("a plan projection that disagrees with the buffer version fails closed");
    assert!(
        error.to_string().contains("disagrees"),
        "the fail-closed diagnostic names the plan-vs-buffer mismatch: {error}"
    );
}

/// A reduced element count that is not a multiple of `inner_stride`
/// cannot address the buffer.
#[test]
fn non_divisible_reduced_element_count_rejected() {
    let mut wire = reduced_projection_fixture();
    wire.kernels[0].resources[1].version.element_count = 9;
    let error = validate_wire_program(&wire)
        .expect_err("a non-divisible reduced element count fails the projection admission");
    assert!(
        error.to_string().contains("not divisible"),
        "the fail-closed diagnostic names the divisibility rule: {error}"
    );
}

/// Axis-reduction plans admit `Sum` only; other ops would pass decode
/// and hit an unimplemented emitter.
#[test]
fn non_sum_axis_reduction_op_rejected() {
    let mut wire = reduced_projection_fixture();
    let WireCollectionKernelPlan::AxisReduction(plan) = &mut wire.kernels[0].plan else {
        panic!("fixture carries an axis-reduction plan");
    };
    plan.op = WireReduceOp::Mean;
    let error = validate_wire_program(&wire)
        .expect_err("a non-Sum axis-reduction op fails the admission closed");
    assert!(
        error.to_string().contains("only Sum"),
        "the fail-closed diagnostic names the op allow-list: {error}"
    );
}

/// An overflowing `axis_extent · inner_stride` fails closed. Plan and
/// buffer/result projections are stamped identically so agreement does
/// not shadow saturation.
#[test]
fn saturated_overflowing_projection_rejected() {
    let mut wire = reduced_projection_fixture();
    let WireCollectionKernelPlan::AxisReduction(plan) = &mut wire.kernels[0].plan else {
        panic!("fixture carries an axis-reduction plan");
    };
    let saturated = WireReducedProjection {
        axis_extent: u64::MAX,
        inner_stride: 2,
    };
    plan.projection = saturated;
    wire.kernels[0].resources[1].version.reduced_projection = Some(saturated);
    wire.kernels[0].resources[1].version.element_count = u64::MAX;
    wire.results[0].version.reduced_projection = Some(saturated);
    wire.results[0].version.element_count = u64::MAX;
    let error = validate_wire_program(&wire)
        .expect_err("an overflowing block width fails the projection admission");
    assert!(
        error.to_string().contains("saturates u64"),
        "the fail-closed diagnostic names the saturation rule: {error}"
    );
}

/// A zero-element reduced buffer cannot hold a reduction output and
/// would pass divisibility vacuously.
#[test]
fn zero_element_reduced_buffer_rejected() {
    let mut wire = reduced_projection_fixture();
    wire.kernels[0].resources[1].version.element_count = 0;
    let error = validate_wire_program(&wire)
        .expect_err("a zero-element reduced buffer fails the projection admission");
    assert!(
        error.to_string().contains("zero-element"),
        "the fail-closed diagnostic names the degenerate-count rule: {error}"
    );
}
