use super::{
    tensor_package_proof_rows, TensorPackageProofTarget, TENSOR_PACKAGE_PROOF_FIXTURE,
    TENSOR_PACKAGE_PROOF_STDOUT,
};
use crate::exempla_e2e::common::make_temp_root;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn tensor_package_rows_cover_fmir_package_targets() {
    let rows = tensor_package_proof_rows();

    for target in [
        TensorPackageProofTarget::FmirText,
        TensorPackageProofTarget::Fmir,
        TensorPackageProofTarget::FmirBin,
    ] {
        assert!(
            rows.iter().any(|row| row.target == target
                && row.fixture_path == TENSOR_PACKAGE_PROOF_FIXTURE
                && row.expected_stdout == TENSOR_PACKAGE_PROOF_STDOUT),
            "missing tensor package proof row for {target:?}: {rows:?}"
        );
    }
}

#[test]
fn tensor_package_runs_through_fmir_targets_without_rust_fallback() {
    for row in tensor_package_proof_rows() {
        let package = copy_tensor_package_fixture(row.target);
        let output = run_faber_package(row.target, &package);

        assert!(
            output.status.success(),
            "faber run --target {} failed\nstdout:\n{}\nstderr:\n{}",
            row.target.cli_target(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), row.expected_stdout);
        assert!(
            package.join(row.target.artifact_path()).exists(),
            "expected {} artifact at {}",
            row.target.cli_target(),
            package.join(row.target.artifact_path()).display()
        );
        assert!(
            !package.join("target/faber/Cargo.toml").exists(),
            "FMIR tensor package proof must not emit generated Rust package output"
        );
    }
}

fn copy_tensor_package_fixture(target: TensorPackageProofTarget) -> PathBuf {
    let fixture = crate::paths::corpus_dir().join(TENSOR_PACKAGE_PROOF_FIXTURE);
    let package = make_temp_root().join(format!("tensor-package-{}", target.cli_target()));
    copy_dir(&fixture, &package);
    package
}

fn copy_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture destination");
    for entry in fs::read_dir(source).expect("read fixture source") {
        let entry = entry.expect("read fixture entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).expect("copy fixture file");
        }
    }
}

fn run_faber_package(target: TensorPackageProofTarget, package: &Path) -> std::process::Output {
    let faber_manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    Command::new("cargo")
        .args([
            "run",
            "-q",
            "--manifest-path",
            faber_manifest.to_str().expect("utf-8 faber manifest path"),
            "--",
            "run",
            "--target",
            target.cli_target(),
        ])
        .arg(package)
        .output()
        .expect("run faber package")
}
