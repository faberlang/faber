//! NGAB1-U1/U2 vertical-slice tests: one analyzed package derives a typed
//! host/device partition and its boundary call lowers through the existing
//! llvm-host path; the versioned call ABI rejects invalid cross-boundary
//! values at compile time with typed diagnostics.
//!
//! The fixture is the frozen NGAB0-U11 shape (`ngab0-fixture-contract.md`):
//! one scalar host function `run_scale(x: f32) -> f32` calling one device
//! kernel `scale_kernel(x: f32) -> f32` through the versioned typed boundary.
//!
//! Evidence rows covered here (frozen contract): the **partition** (row 1 —
//! one host function, one prepared submission region, one device kernel, one
//! boundary call — the minimal one-prepared-region case, NGAB0-R1), the
//! **typed device program** (identity, resources, launches, lifetimes,
//! observations — all typed facts, never text-parsed), the **host-module
//! emission** of the boundary call through the llvm-host path
//! (`build_package_llvm`), and NGAB1-U2's **versioned call ABI** (the
//! boundary rides `HOST_CALL_ABI_VERSION`, asserted against the accepted wire
//! version) with its **compile-time rejection** of invalid cross-boundary
//! values (wrong type, shape, lifetime, mutation of a read-only resource,
//! observation of unlaunched work) — typed diagnostics, no launch.
//!
//! The host-partition lane (DDCP3-U2) emits the CPU module for a package
//! that carries device kernels: device-role entries emit as direct
//! declarations and host boundary calls emit as direct calls to
//! prepared-region symbols — physical kernel bodies and the region's device
//! work belong to a device backend ("no physical execution" is the lane's
//! frozen non-goal), so the llvm-host build proof is the emitted host-module
//! contract, not a CPU runtime execution of the kernel.
//!
//! The toolchain-dependent emission proof runs only when a coherent LLVM
//! toolchain is discoverable (the `llvm_host_test.rs` convention).

use super::*;
use crate::package::device::DEVICE_RUN_PLAN_VERSION;
use crate::package::test_support::{TestDir, test_temp_dir};
use radix::module::codegen::Target;
use radix_mir::boundary_abi::HOST_CALL_ABI_VERSION;
use radix_mir::device_program::{
    BufferLifetime, BufferRole, DeviceProgramLifetime, ObservationCadence,
};
use radix_mir::kernel_plan::CollectionKernelPlan;
use radix_mir::layout::MirTensorStorageLayout;
use std::fs;
use std::path::PathBuf;

/// The NGAB0-U11 fixture — one scalar host function calling one device
/// kernel, plus the entry that exercises the call at the declared input.
/// `fractus<f32>` is the f32 scalar surface (plain `fractus` is f64).
const NGAB0_U11_FIXTURE: &str = r#"@ nucleum
functio scale_kernel(f32 x) → f32 {
    redde x * 2.0 + 1.0
}

functio run_scale(f32 x) → f32 {
    redde scale_kernel(x)
}

incipit {
    nota run_scale(3.0 ∷ f32)
}"#;

fn llvm_host_config() -> radix::module::Config {
    radix::module::Config::default().with_target(Target::MirLlvmHost)
}

fn toolchain_available() -> bool {
    radix::module::llvm_host::LlvmHostToolchain::discover().is_ok()
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

        // One declared boundary call: a host function → the prepared region
        // containing the device kernel (NGAB0-R1: one host call → one
        // prepared submission region; the one-kernel region is the minimal
        // case, not a separate ABI shape).
        assert_eq!(partition.boundary_calls.len(), 1);
        assert_eq!(
            partition.boundary_calls[0].region.kernels,
            vec![scale_kernel_id]
        );
        assert!(
            partition.boundary_calls[0].region.is_minimal(),
            "the fixture's prepared region is the minimal one-kernel case"
        );
        assert_ne!(
            partition.boundary_calls[0].host, scale_kernel_id,
            "the boundary call's caller is a host function"
        );

        // The versioned typed boundary (NGAB1-U2): the partition carries the
        // call-ABI ratchet; the faber wire seam asserts it rides the accepted
        // device-program wire version.
        assert_eq!(partition.call_abi_version, HOST_CALL_ABI_VERSION);

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
    // The host-partition lane (DDCP3-U2) emits the CPU side of a device
    // package: device-role entries become direct declarations and the host
    // boundary call becomes a direct call to the prepared-region symbol with
    // a bounded numeric dispatch extent. Physical kernel bodies and the
    // region's device work are provided by a device backend, never by this
    // CPU module ("no physical execution" is the lane's frozen non-goal), so
    // the build proof here is the emitted host module contract — the same
    // contract the radix-mir-llvm vertical fixture proves.
    let build = radix::program::llvm::build_package_llvm(
        &llvm_host_config(),
        &entry,
        &radix::program::llvm::PackageLlvmOptions::new(
            dir.as_ref().join("target/faber-llvm/debug/modules"),
        ),
        &crate::package::faber_analysis_seams(),
    )
    .expect("llvm modules build succeeds for the host-plus-device fixture");

    let entry_module = build
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("the fixture emits exactly one entry module")
        .llvm_path
        .clone();
    let module_text = std::fs::read_to_string(&entry_module).expect("read entry module");

    // The lane declares the prepared-region symbol (one per prepared region)
    // and the host boundary call is a direct call to it with the typed
    // dispatch extent — never a call to the kernel's body symbol.
    assert!(
        module_text.contains("declare void @__faber_prepared_region_1(i64)"),
        "the CPU module declares the prepared-region symbol; got:\n{module_text}"
    );
    assert!(
        module_text.lines().any(|line| {
            line.trim_start()
                .starts_with("call void @__faber_prepared_region_1(i64 ")
        }),
        "the host boundary call is a direct call to the prepared-region symbol; got:\n{module_text}"
    );

    // No physical device kernel body rides the CPU module: the kernel symbol
    // is never defined and no device-entry annotations appear.
    assert!(
        !module_text.contains("@nucleum"),
        "the CPU module carries no device-entry annotations; got:\n{module_text}"
    );
    let kernel_symbol = "__faber_external_product_fixture_scalar_kernel_module_fixture_scalar_kernel_func_scale_kernel";
    let kernel_has_define = module_text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("define") && trimmed.contains(kernel_symbol)
    });
    assert!(
        !kernel_has_define,
        "the CPU module has no `define` body for the device kernel `{kernel_symbol}`; got:\n{module_text}"
    );
}

// ── NGAB1-U2: versioned call ABI + compile-time rejection ─────────────────

/// The versioned boundary rides the accepted wire version (NGAB0 §Abi: the
/// boundary is a versioned surface with its own ratchet that rides the
/// accepted wire version — no `WIRE_DEVICE_PROGRAM_VERSION` bump; the MD2-W1
/// sibling-field precedent).
#[test]
fn ngab1_call_abi_rides_the_accepted_wire_version() {
    assert_eq!(
        HOST_CALL_ABI_VERSION, DEVICE_RUN_PLAN_VERSION,
        "the versioned call ABI rides the accepted device-program wire version"
    );
    assert_eq!(HOST_CALL_ABI_VERSION, 8);
}

/// Lower a negative fixture and assert the boundary ABI rejects it at compile
/// time with the typed `E_DEVICE_BOUNDARY_ABI` diagnostic of the expected
/// class. The fixture must pass source lowering + general MIR validation
/// (rejection is the *boundary* gate's, not a parse/type error), and nothing
/// is built or launched.
fn expect_boundary_rejection(source: &str, class: &'static str) {
    let dir = test_temp_dir("ngab1-boundary-negative");
    let package_dir = dir.join("fixture-scalar-kernel");
    fs::create_dir_all(&package_dir).expect("create fixture dir");
    let entry = package_dir.join("fixture-scalar-kernel.fab");
    fs::write(&entry, source).expect("write fixture");
    let rejection = with_lowered_package_mir(&llvm_host_config(), &entry, |lowered| {
        host_partition_for_lowered(lowered)
    });
    match rejection {
        Ok(Err(diagnostics)) => {
            let boundary = diagnostics.iter().find(|diag| {
                diag.args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "E_DEVICE_BOUNDARY_ABI")
            });
            let boundary = boundary.unwrap_or_else(|| {
                panic!("expected an E_DEVICE_BOUNDARY_ABI diagnostic; got {diagnostics:?}")
            });
            assert!(
                boundary
                    .args
                    .iter()
                    .any(|arg| arg.name == "class" && arg.value == class),
                "expected the {class} boundary class; got {boundary:?}"
            );
        }
        Ok(Ok(_)) => panic!(
            "the negative fixture must be rejected at compile time (class {class}); the partition derived"
        ),
        Err(diagnostics) => panic!(
            "the negative fixture must pass lowering/general validation (rejection is the boundary gate's); got {diagnostics:?}"
        ),
    }
}

/// Wrong type: an `fractus<f16>` value crosses a boundary that declares
/// `fractus<f32>`. General MIR allows the widening; the versioned typed
/// boundary requires the exact carried element type fact (every crossing
/// value carries its type fact — §Abi).
#[test]
fn ngab1_boundary_rejects_wrong_type() {
    expect_boundary_rejection(
        r#"@ nucleum
functio scale_kernel(f32 x) → f32 {
    redde x * 2.0 + 1.0
}

functio run_scale(f16 x) → f32 {
    redde scale_kernel(x)
}

incipit {
    nota run_scale(1.0 ∷ f16)
}"#,
        "type",
    );
}

/// Wrong lifetime: the boundary call's output must not land in caller-owned
/// parameter storage. At the source level this pattern is already blocked by
/// the semantic analyzer (SEM020 — parameters are immutable), so the faber
/// fixture cannot express it; the typed `Lifetime` diagnostic of the boundary
/// gate is proven at the MIR level by `radix-mir`'s
/// `boundary_abi_rejects_output_into_host_parameter` unit test. The gate
/// still guards MIR-level producers that write a call's output into a host
/// parameter (`Assign { place: param, value: <call output> }`).
#[test]
fn ngab1_boundary_rejects_output_into_parameter_guarded() {
    // The source-language guard fires first: reassigning a parameter is
    // SEM020 (immutable second assignment), a compile-time rejection without
    // a launch — the same outcome the boundary gate produces for MIR-level
    // producers.
    let dir = test_temp_dir("ngab1-boundary-lifetime");
    let package_dir = dir.join("fixture-scalar-kernel");
    fs::create_dir_all(&package_dir).expect("create fixture dir");
    let entry = package_dir.join("fixture-scalar-kernel.fab");
    fs::write(
        &entry,
        r#"@ nucleum
functio scale_kernel(f32 x) → f32 {
    redde x * 2.0 + 1.0
}

functio run_scale(f32 x) → f32 {
    x ← scale_kernel(1.0 ∷ f32)
    redde x
}

incipit {
    nota run_scale(3.0 ∷ f32)
}"#,
    )
    .expect("write fixture");
    let rejection = with_lowered_package_mir(&llvm_host_config(), &entry, |lowered| {
        host_partition_for_lowered(lowered)
    });
    match rejection {
        Err(diagnostics) => {
            assert!(
                diagnostics
                    .iter()
                    .any(|diag| diag.message.contains("immutable_second_assignment")),
                "the source-level lifetime guard rejects writing into the parameter; got {diagnostics:?}"
            );
        }
        Ok(Ok(_)) | Ok(Err(_)) => {
            panic!(
                "the lifetime-negative fixture must be rejected at compile time (SEM020) without a launch"
            )
        }
    }
}

/// Mutation of a read-only resource: the call writes its output into the same
/// storage as its input value — the device input slot is read-only.
#[test]
fn ngab1_boundary_rejects_in_place_mutation() {
    expect_boundary_rejection(
        r#"@ nucleum
functio scale_kernel(f32 x) → f32 {
    redde x * 2.0 + 1.0
}

functio run_scale(f32 x) → f32 {
    varia f32 y ← x
    y ← scale_kernel(y)
    redde y
}

incipit {
    nota run_scale(3.0 ∷ f32)
}"#,
        "mutation",
    );
}

/// Observation of unlaunched work: a second call passes the first call's
/// output (the kernel's region result) back in as an input; the program's
/// single launch for the kernel is both producer and consumer.
#[test]
fn ngab1_boundary_rejects_observing_unlaunched_work() {
    expect_boundary_rejection(
        r#"@ nucleum
functio scale_kernel(f32 x) → f32 {
    redde x * 2.0 + 1.0
}

functio run_scale(f32 x) → f32 {
    varia f32 y ← scale_kernel(x)
    redde scale_kernel(y)
}

incipit {
    nota run_scale(3.0 ∷ f32)
}"#,
        "observation",
    );
}

// ── NGAB1-U4: batch compatible call shapes ────────────────────────────────
//
// After the first accepted call shape (the scalar), compatible shapes batch
// through the same ABI without new mechanisms. Batched shapes are MULTIPLE
// KERNELS PER PREPARED SUBMISSION REGION — one host call → one region
// containing the batch's kernels, never per-kernel ABI rows (NGAB0-R1: the
// region is the call's submission unit, not the kernel).

/// The two-kernel composition (scale → offset, the first kernel's output
/// feeding the second kernel's input) lowers through the real product path
/// and derives as ONE boundary call → ONE prepared submission region carrying
/// the batch's two kernels in invocation order — no per-kernel ABI rows.
/// Both kernels' typed facts (entry identity, ordered launches, declared
/// observations) survive into the derived device program.
const NGAB1_U4_TWO_KERNEL_FIXTURE: &str = r#"@ nucleum
functio scale_kernel(f32 x) → f32 {
    redde x * 2.0 + 1.0
}

@ nucleum
functio offset_kernel(f32 x) → f32 {
    redde x + 5.0
}

functio run_batch(f32 x) → f32 {
    varia f32 y ← scale_kernel(x)
    redde offset_kernel(y)
}

incipit {
    nota run_batch(3.0 ∷ f32)
}"#;

#[test]
fn ngab1_two_kernel_composition_derives_one_region_per_call() {
    let dir = test_temp_dir("ngab1-two-kernel-composition");
    let package_dir = dir.join("fixture-scalar-kernel");
    fs::create_dir_all(&package_dir).expect("create fixture dir");
    let entry = package_dir.join("fixture-scalar-kernel.fab");
    fs::write(&entry, NGAB1_U4_TWO_KERNEL_FIXTURE).expect("write fixture");

    with_lowered_package_mir(&llvm_host_config(), &entry, |lowered| {
        let partition = host_partition_for_lowered(lowered)
            .expect("host partition derives")
            .expect("the fixture carries a device program");

        // Two device kernels (scale → offset in declaration order); the host
        // side is every other function.
        assert_eq!(partition.device_kernels.len(), 2);
        let scale_id = partition.device_kernels[0];
        let offset_id = partition.device_kernels[1];
        assert!(!partition.host_functions.contains(&scale_id));
        assert!(!partition.host_functions.contains(&offset_id));

        // ONE declared boundary call → ONE prepared region carrying the
        // batch's two kernels in invocation order (never per-kernel rows).
        assert_eq!(partition.boundary_calls.len(), 1);
        let call = &partition.boundary_calls[0];
        assert_ne!(
            call.host, scale_id,
            "the boundary call's caller is a host function"
        );
        assert_eq!(call.region.kernels, vec![scale_id, offset_id]);
        assert!(
            !call.region.is_minimal(),
            "the batch region carries the composition's two kernels"
        );
        assert_eq!(partition.call_abi_version, HOST_CALL_ABI_VERSION);

        // Both kernels derive with their typed facts: entry identity, ordered
        // launches, declared observations.
        let program = &partition.device_program;
        assert_eq!(program.kernels.len(), 2);
        assert_eq!(program.kernels[0].entry, "scale_kernel");
        assert_eq!(program.kernels[1].entry, "offset_kernel");
        assert_eq!(program.launches.len(), 2);
        assert_eq!(program.launches[0].kernel_index, 0);
        assert_eq!(program.launches[1].kernel_index, 1);
        assert_eq!(program.results.len(), 2);
        assert_eq!(program.results[0].produced_by, program.launches[0].id);
        assert_eq!(program.results[1].produced_by, program.launches[1].id);
        program
            .validate()
            .expect("derived program passes the shared schema");
    })
    .expect("fixture lowers and analyzes");
}
