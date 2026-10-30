//! Behavioural coverage for package MIR run/build routes.

use super::*;
use radix_mir_runner::BufferHost;
use std::path::{Path, PathBuf};

fn package(label: &str, source: &str) -> (crate::test_support::TestDir, PathBuf) {
    let dir = crate::test_support::test_temp_dir(label);
    let entry = dir.join("main.fab");
    std::fs::write(&entry, source).expect("write package entry");
    (dir, entry)
}

fn carries_device(
    _prepared: &PreparedPackageMir<'_>,
    _lowered: &radix_mir_lower::lower::LoweredMirUnit<'_>,
    _path: &Path,
) -> Result<Option<radix_mir_fmir::FmirDeviceSection>, Vec<Diagnostic>> {
    Ok(Some(radix_mir_fmir::FmirDeviceSection {
        device_program: radix_mir_fmir::FmirDeviceProgramSection {
            v: 1,
            program: radix_mir_fmir::WireDeviceProgram::default(),
        },
        executor_plan: None,
        selection: radix_mir_fmir::FmirDeviceSelection::Metal,
        artifacts: radix_mir_fmir::FmirDeviceArtifactsSection::default(),
        declared_inputs: Vec::new(),
        distributed: None,
        session: None,
    }))
}

#[test]
fn binary_image_build_decision_and_cpu_runner_preserve_program_output() {
    let (_dir, entry) = package("routes-binary-image", "main {\n    print \"binary\"\n}\n");
    let seams = crate::test_support::test_mir_seams();
    let image = build_package_fmir_image(&Config::default(), &entry, &[], &seams)
        .expect("build binary image");

    let decision = fmir_image_route_decision(&image.image_path, &seams).expect("route decision");
    assert!(!decision.requires_device);
    assert_eq!(decision.declared_selection, DeviceSelection::Auto);
    assert!(decision.declared_artifacts.is_empty());

    let mut host = BufferHost::default();
    run_package_fmir_image_with_selection(&image, DeviceSelection::Reference, &seams, &mut host)
        .expect("run a CPU-only image with an explicit host selection");
    assert_eq!(host.stdout_lines, vec!["binary".to_owned()]);
}

#[test]
fn text_image_route_runs_and_a_missing_image_is_refused_before_output() {
    let (_dir, entry) = package("routes-text-image", "main {\n    print \"text\"\n}\n");
    let seams = crate::test_support::test_mir_seams();
    let image = build_package_fmir_text_image(&Config::default(), &entry, &[], &seams)
        .expect("build text image");
    let mut host = BufferHost::default();
    run_package_fmir_text_image(&image, &seams, &mut host).expect("run text image");
    assert_eq!(host.stdout_lines, vec!["text".to_owned()]);

    let missing = PackageFmirTextImage {
        image_path: image.image_path.with_extension("missing"),
    };
    let mut host = BufferHost::default();
    assert!(run_package_fmir_text_image(&missing, &seams, &mut host).is_err());
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn package_artifact_route_checks_manifest_then_runs_its_entry() {
    let (_dir, entry) = package(
        "routes-package-artifact",
        "main {\n    print \"artifact\"\n}\n",
    );
    let seams = crate::test_support::test_mir_seams();
    let artifact = build_package_mir_artifact(&Config::default(), &entry, &[], &seams)
        .expect("build package MIR artifact");
    let mut host = BufferHost::default();
    run_package_mir_artifact(&Config::default(), &artifact, &seams, &mut host)
        .expect("run artifact entry");
    assert_eq!(host.stdout_lines, vec!["artifact".to_owned()]);

    std::fs::write(&artifact.manifest_path, "version = 0\n").expect("corrupt manifest");
    let diagnostics = run_package_mir_artifact(&Config::default(), &artifact, &seams, &mut host)
        .expect_err("manifest without the required artifact metadata must be rejected");
    assert!(
        diagnostics
            .iter()
            .any(|diag| { diag.issue() == Some("package_mir_artifact_manifest_metadata_missing") })
    );
}

#[test]
fn interpreted_bound_route_binds_host_arguments_before_calling_the_consumer() {
    let (_dir, entry) = package(
        "routes-interpreted-bound",
        "@ cli \"greet\"\n@ operand string name\nmain args args {\n    print args.name\n}\n",
    );
    let mut host = BufferHost::with_argumenta(vec!["Ada".to_owned()]);
    with_interpreted_bound_package_mir(
        &Config::default(),
        &entry,
        &mut host,
        &crate::test_support::test_mir_seams(),
        |_lowered, validated, exit_code, host| {
            assert_eq!(exit_code, None);
            radix_mir_runner::run_entry(validated, host).expect("run bound entry");
            Ok(())
        },
    )
    .expect("bind and run CLI arguments");
    assert_eq!(host.stdout_lines, vec!["Ada".to_owned()]);
}

#[test]
fn device_bearing_package_is_refused_by_auto_but_cpu_selection_runs_host_form() {
    let (_dir, entry) = package("routes-device-policy", "main {\n    print \"host\"\n}\n");
    let mut seams = crate::test_support::test_mir_seams();
    seams.device_section_builder = carries_device;

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir_with_selection(
        &Config::default(),
        &entry,
        DeviceSelection::Auto,
        &seams,
        &mut host,
    )
    .expect_err("a device-bearing image cannot use the retired device runner route");
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("E_DEVICE_IMAGE_ROUTE_RETIRED"))
    );
    assert!(host.stdout_lines.is_empty());

    run_package_mir_with_selection(
        &Config::default(),
        &entry,
        DeviceSelection::Cpu,
        &seams,
        &mut host,
    )
    .expect("CPU selection runs the host-form runner even for a device-bearing package");
    assert_eq!(host.stdout_lines, vec!["host".to_owned()]);
}

#[test]
fn interpreted_bound_route_refuses_missing_required_cli_operands() {
    let (_dir, entry) = package(
        "routes-interpreted-missing-operand",
        "@ cli \"greet\"\n@ operand string name\nmain args args {\n    print args.name\n}\n",
    );
    let mut host = BufferHost::default();
    let mut callback_called = false;
    let result = with_interpreted_bound_package_mir(
        &Config::default(),
        &entry,
        &mut host,
        &crate::test_support::test_mir_seams(),
        |_lowered, _validated, _exit_code, _host| {
            callback_called = true;
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(
        !callback_called,
        "invalid argv must fail before consumer execution"
    );
}

#[test]
fn explicit_gpu_selection_on_a_cpu_only_package_fails_closed() {
    let (_dir, entry) = package(
        "routes-explicit-gpu",
        "main {\n    print \"unreachable\"\n}\n",
    );
    let mut host = BufferHost::default();
    let diagnostics = run_package_mir_with_selection(
        &Config::default(),
        &entry,
        DeviceSelection::Cuda,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect_err("explicit device selection must not silently fall back");
    assert!(diagnostics.iter().any(|diag| {
        diag.args
            .iter()
            .any(|arg| arg.name == "selection" && arg.value == "cuda")
    }));
    assert!(host.stdout_lines.is_empty());
}

const EVM_CONTRACT: &str = r#"
@ contract { target = "evm" }
class Arithmetic {
    @ public
    fn add(int a, int b) → int { return a + b }
}
main { print 7 }
"#;

fn assert_contract_target_mismatch(diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == Some("PKG001")
                && diagnostic.issue() == Some("contract_target_mismatch")
                && diagnostic
                    .args
                    .iter()
                    .any(|arg| arg.name == "required" && arg.value == "evm")
        }),
        "expected contract environment refusal: {diagnostics:?}"
    );
}

#[test]
fn contract_execution_requirement_is_enforced_at_package_run_and_artifact_preparation() {
    let (_dir, entry) = package("routes-contract-environment", EVM_CONTRACT);
    let seams = crate::test_support::test_mir_seams();
    let config = Config::default();
    let mut host = BufferHost::default();
    let diagnostics = run_package_mir(&config, &entry, &seams, &mut host)
        .expect_err("an EVM contract cannot execute on the MIR interpreter");
    assert_contract_target_mismatch(&diagnostics);
    assert!(host.stdout_lines.is_empty());
    let diagnostics = build_package_fmir_image(&config, &entry, &[], &seams)
        .err()
        .expect("an executable FMIR image cannot ignore an EVM requirement");
    assert_contract_target_mismatch(&diagnostics);
    let diagnostics = build_package_fmir_text_image(&config, &entry, &[], &seams)
        .err()
        .expect("an executable FMIR text image cannot ignore an EVM requirement");
    assert_contract_target_mismatch(&diagnostics);
}

#[test]
fn contract_metadata_survives_generic_package_check_and_mir_inspection() {
    for source in [
        EVM_CONTRACT.to_owned(),
        EVM_CONTRACT.replace("@ contract { target = \"evm\" }\n", ""),
    ] {
        let (_dir, entry) = package("routes-contract-inspection", &source);
        let seams = crate::test_support::test_mir_seams();
        let config = Config::default();
        crate::analyze::analyze_package(&config, &entry, &seams.analysis)
            .unwrap_or_else(|errors| panic!("generic checking retains class metadata: {errors:?}"));
        with_check_lowered_package_mir(&config, &entry, &seams, |_| ())
            .unwrap_or_else(|errors| panic!("check --lower is inspection: {errors:?}"));
        with_lowered_package_mir(&config, &entry, &seams, |_| ())
            .unwrap_or_else(|errors| panic!("a target probe is inspection: {errors:?}"));
    }
}

#[test]
fn contract_execution_requirement_is_enforced_before_test_linking() {
    let (_dir, entry) = package("routes-contract-test-link", EVM_CONTRACT);
    let seams = crate::test_support::test_mir_seams();
    let config = Config::default();
    let package = crate::analyze::analyze_package(&config, &entry, &seams.analysis)
        .expect("generic package analysis");
    let scope = TestPackageScope::for_program(&package, Vec::new());
    let diagnostics = with_linked_test_package_mir(package, &scope, &config, &seams, |_, _| ())
        .expect_err("test execution cannot ignore an EVM environment");
    assert_contract_target_mismatch(&diagnostics);
}
