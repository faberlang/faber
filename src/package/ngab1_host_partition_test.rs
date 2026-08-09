//! NGAB1-U1 vertical-slice tests: one analyzed package derives a typed
//! host/device partition AND executes its boundary call through the existing
//! llvm-host path.
//!
//! The fixture is the frozen NGAB0-U11 shape (`ngab0-fixture-contract.md`):
//! one scalar host function `run_scale(x: f32) -> f32` calling one device
//! kernel `scale_kernel(x: f32) -> f32` through the versioned typed boundary.
//! The CPU oracle is `oracle(x) = x * 2.0 + 1.0` (owned by this test harness),
//! exercised at the declared input `x = 3.0` → `7.0`.
//!
//! Evidence rows covered here (frozen contract): the **partition** (row 1 —
//! one host function, one device kernel, one boundary call), the **typed
//! device program** (identity, resources, launches, lifetimes, observations —
//! all typed facts, never text-parsed), and the **execution** of the call
//! through the existing llvm-host path (`build_host_program`). The
//! toolchain-dependent execution proof runs only when a coherent LLVM
//! toolchain is discoverable (the `llvm_host_test.rs` convention).

use super::*;
use crate::package::test_support::{test_temp_dir, TestDir};
use radix::codegen::Target;
use radix_mir::device_program::{
    BufferLifetime, BufferRole, DeviceProgramLifetime, ObservationCadence,
};
use radix_mir::kernel_plan::CollectionKernelPlan;
use radix_mir::layout::MirTensorStorageLayout;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// The NGAB0-U11 fixture — one scalar host function calling one device
/// kernel, plus the entry that exercises the call at the declared input.
/// `fractus<f32>` is the f32 scalar surface (plain `fractus` is f64).
const NGAB0_U11_FIXTURE: &str = r#"@ nucleum
functio scale_kernel(fractus<f32> x) → fractus<f32> {
    redde x * 2.0 + 1.0
}

functio run_scale(fractus<f32> x) → fractus<f32> {
    redde scale_kernel(x)
}

incipit {
    nota run_scale(3.0 ∷ fractus<f32>)
}"#;

/// The CPU oracle (frozen): `oracle(x) = x * 2.0 + 1.0`.
fn oracle(x: f32) -> f32 {
    x * 2.0 + 1.0
}

fn llvm_host_config() -> radix::Config {
    radix::Config::default().with_target(Target::MirLlvmHost)
}

fn toolchain_available() -> bool {
    radix::llvm_host::LlvmHostToolchain::discover().is_ok()
}

/// Write the single-unit fixture under a temp package dir and return the
/// `.fab` entry path. The directory name is fixed so the product identity is
/// deterministic.
fn write_fixture(dir: &TestDir) -> PathBuf {
    let package_dir = dir.join("fixture-scalar-kernel");
    fs::create_dir_all(&package_dir).expect("create fixture dir");
    let entry = package_dir.join("fixture-scalar-kernel.fab");
    fs::write(&entry, NGAB0_U11_FIXTURE).expect("write fixture");
    entry
}

// ── Partition + typed device program (always runs) ─────────────────────────

#[test]
fn ngab1_host_partition_derives_typed_device_program() {
    let dir = test_temp_dir("ngab1-host-partition");
    let entry = write_fixture(&dir);
    with_lowered_package_mir(&llvm_host_config(), &entry, |lowered| {
        let partition = host_partition_for_lowered(lowered)
            .expect("host partition derives")
            .expect("the fixture carries a device program");
        let program = &partition.device_program;

        // ── Partition (evidence row 1) ─────────────────────────────────────
        // One device kernel; the host side is every other function.
        assert_eq!(partition.device_kernels.len(), 1);
        let scale_kernel_id = partition.device_kernels[0];
        assert!(
            !partition.host_functions.contains(&scale_kernel_id),
            "the kernel must not be on the host side"
        );
        assert!(
            partition.host_functions.len() >= 2,
            "host side carries run_scale + the entry"
        );

        // One declared boundary call: a host function → the device kernel.
        assert_eq!(partition.boundary_calls.len(), 1);
        assert_eq!(partition.boundary_calls[0].kernel, scale_kernel_id);
        assert_ne!(
            partition.boundary_calls[0].host, scale_kernel_id,
            "the boundary call's caller is a host function"
        );

        // ── Typed device-program facts (never text-parsed) ────────────────
        program
            .validate()
            .expect("derived device program passes shared schema validation");
        assert_eq!(program.lifetime, DeviceProgramLifetime::SingleRun);
        assert_eq!(program.kernels.len(), 1);
        assert_eq!(program.launches.len(), 1);

        let kernel = &program.kernels[0];
        // Identity: the kernel's MIR function id + logical entry name.
        assert_eq!(kernel.function, scale_kernel_id);
        assert_eq!(kernel.entry, "scale_kernel");
        // Plan: a scalar body has no recipe op → Elementwise.
        assert_eq!(kernel.plan, CollectionKernelPlan::Elementwise);

        // Resources: two typed storage-buffer slots — a 1-element input at
        // binding 0 and a 1-element output at binding 1.
        assert_eq!(kernel.resources.len(), 2);
        let input = &kernel.resources[0];
        let output = &kernel.resources[1];
        assert_eq!(input.buffer.role, BufferRole::Input);
        assert_eq!(input.buffer.lifetime, BufferLifetime::PerProgram);
        assert_eq!(input.buffer.storage, MirTensorStorageLayout::DeviceHandle);
        assert_eq!(input.binding.binding, 0);
        assert_eq!(input.version.element_count, 1);
        assert_eq!(output.buffer.role, BufferRole::Output);
        assert_eq!(output.binding.binding, 1);
        assert_eq!(output.version.element_count, 1);
        // Both slots carry the same scalar element type — the f32 fact.
        assert_eq!(
            input.version.element_ty, output.version.element_ty,
            "scalar in/out ride one element type"
        );
        let element_layout = lowered
            .validated
            .validation()
            .layouts
            .layout_for_type(input.version.element_ty)
            .expect("element type has a layout");
        assert!(
            matches!(
                element_layout.kind,
                radix_mir::MirLayoutKind::Scalar(radix_mir::MirScalarLayout::F32)
            ),
            "the fixture's scalar element type is f32"
        );

        // Launches: one ordered launch over the kernel.
        assert_eq!(program.launches[0].kernel_index, 0);
        let launch_id = program.launches[0].id;

        // Observations: one explicit EndOfRun result for the output buffer,
        // produced by the single launch — a declared readback, never inferred.
        assert_eq!(program.results.len(), 1);
        let result = &program.results[0];
        assert_eq!(result.role, BufferRole::Output);
        assert_eq!(result.buffer.role, BufferRole::Output);
        assert_eq!(result.buffer.id, output.buffer.id);
        assert_eq!(result.produced_by, launch_id);
        assert_eq!(result.cadence, ObservationCadence::EndOfRun);
    })
    .expect("fixture lowers and analyzes");
}

// ── Execution through the existing llvm-host path (needs llvm-as/clang) ────

#[test]
fn ngab1_host_partition_executes_boundary_call_via_llvm_host() {
    if !toolchain_available() {
        eprintln!("ngab1 test skipped: coherent LLVM toolchain not available");
        return;
    }
    let dir = test_temp_dir("ngab1-llvm-host");
    let entry = write_fixture(&dir);
    let build = build_host_program(&llvm_host_config(), &entry, LlvmHostProfile::Debug)
        .expect("llvm-host build succeeds for the host-plus-device fixture");

    // The boundary call must link (one native executable) and execute: the
    // entry prints run_scale(3.0) = scale_kernel(3.0) = 3.0*2.0+1.0.
    let run = Command::new(&build.binary_path)
        .output()
        .expect("run built binary");
    assert!(
        run.status.success(),
        "built binary must exit successfully; stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let expected = format!("{:.1}\n", oracle(3.0));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected,
        "built binary stdout must match the CPU oracle run_scale(3.0) = 7.0"
    );
}
