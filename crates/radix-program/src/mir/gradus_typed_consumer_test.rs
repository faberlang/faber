//! Generated Rust executes real typed Gradus kernels through a consumer package.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use radix_module::Output;
use radix_module::codegen::Target;
use radix_module::driver::Config;

use crate::emit::{EmissionSeams, compile_package};
use crate::test_support::{test_analysis_seams, test_temp_dir};

const CONSUMER_SOURCE: &str = r#"+++
locale = "en"
+++

import from "gradus:math" math
import from "gradus:loss" loss

fn drop_one<size N>(tensor<f32, [N - 1]> value) → tensor<f32, [N - 1]> { return value }
fn half_width<size D>(tensor<f32, [D / 2]> value) → tensor<f32, [D / 2]> { return value }
fn cancel_offset<size N>(tensor<f32, [N - 1 + 1]> value) → tensor<f32, [N - 1 + 1]> { return value }
fn runtime_size(int value) → int { return value }

main {
    const list<f32> data ← [1.0, 2.0, 3.0, 4.0]
    const int n ← data.length() / 2
    const tensor<f32, [size n, size n]> source ← data ↦ tensor<f32, [size n, size n]>
    const tensor<f32, [size n, size n]> materialized ← sourceᵀ.materialize()
    const tensor<f32, [size n, size n]> sum ← math.add(materialized, source)
    const f32 error ← loss.mse(sum, source)
    const list<f32> values ← sum ↦ list<f32>
    print values[0]
    print values[1]
    print values[2]
    print values[3]
    print error
    const list<f32> source_flat ← source.flatten()
    const tensor<f32, [size n, size n]> product ← source · source
    const list<f32> product_values ← product ↦ list<f32>
    print source_flat[0]
    print product_values[0]
    const list<f32> three_values ← [1.0, 2.0, 3.0]
    const tensor<f32, [3]> three ← three_values ↦ tensor<f32, [3]>
    const tensor<f32, [3]> tail ← drop_one(three)
    print tail.shape()[0]
    const list<f32> four_values ← [1.0, 2.0, 3.0, 4.0]
    const tensor<f32, [4]> four ← four_values ↦ tensor<f32, [4]>
    const tensor<f32, [4]> half ← half_width(four)
    print half.shape()[0]
    const list<f32> empty_flat ← []
    const tensor<f32, [0]> zero ← empty_flat ↦ tensor<f32, [0]>
    const tensor<f32, [0]> canceled ← cancel_offset(zero)
    print canceled.shape()[0]
    const int extent ← runtime_size(4)
    const list<f32> dynamic_flat ← [1.0, 2.0, 3.0]
    const tensor<f32, [size (extent - 1)]> dynamic ← dynamic_flat ↦ tensor<f32, [size (extent - 1)]>
    const tensor<f32, [size (extent - 1)]> dynamic_tail ← drop_one(dynamic)
    print dynamic_tail.shape()[0]
}
"#;

struct GeneratedRun {
    status: ExitStatus,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

fn emission_seams() -> EmissionSeams {
    EmissionSeams {
        analysis: test_analysis_seams(),
        #[cfg(feature = "hir-go")]
        emit_go: never_exercised_go_emission,
    }
}

#[cfg(feature = "hir-go")]
fn never_exercised_go_emission(
    _package: &crate::analyze::AnalyzedProgram,
    _input: &Path,
) -> crate::emit::PackageCompileResult {
    unreachable!("typed Gradus consumer test only emits Rust")
}

fn write_consumer_package(root: &Path) {
    let source = root.join("src");
    fs::create_dir_all(&source).expect("create consumer src");
    fs::write(
        root.join("faber.toml"),
        "[package]\nname = \"gradus-typed-consumer\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n\n[locale]\nlocale = \"en\"\n",
    )
    .expect("write consumer manifest");
    fs::write(source.join("main.fab"), CONSUMER_SOURCE).expect("write consumer source");
}

fn generated_run(project: &Path, target: &Path) -> GeneratedRun {
    let stdout_path = project.join("run.stdout");
    let stderr_path = project.join("run.stderr");
    let stdout = File::create(&stdout_path).expect("create generated stdout log");
    let stderr = File::create(&stderr_path).expect("create generated stderr log");
    let manifest = project.join("Cargo.toml");
    let mut child = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .arg("run")
        .arg("--quiet")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(target)
        .current_dir(project)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("start bounded generated-Rust execution");

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll generated cargo process") {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(120) {
            timed_out = true;
            child
                .kill()
                .expect("stop generated cargo process after timeout");
            break child
                .wait()
                .expect("reap timed-out generated cargo process");
        }
        thread::sleep(Duration::from_millis(25));
    };

    GeneratedRun {
        status,
        timed_out,
        stdout: fs::read_to_string(stdout_path).expect("read generated stdout log"),
        stderr: fs::read_to_string(stderr_path).expect("read generated stderr log"),
    }
}

fn runtime_crate_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../faber/runtime/rust")
        .canonicalize()
        .expect("resolve packet-local Faber runtime crate")
}

#[test]
fn generated_rust_executes_gradus_typed_math_and_loss_on_runtime_sized_materialized_tensor() {
    let package = test_temp_dir("gradus-typed-consumer");
    write_consumer_package(package.as_ref());

    let library_home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("resolve packet library home");
    let config = Config::default()
        .with_library_home(library_home)
        .with_target(Target::HirRust);
    let result = compile_package(&config, package.as_ref(), &emission_seams());
    let Some(Output::Rust(output)) = result.output else {
        let diagnostics = result
            .diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "file={:?} line={:?} source={:?} code={:?} issue={:?} message={:?}",
                    diagnostic.file,
                    diagnostic.line,
                    diagnostic.source_line,
                    diagnostic.code,
                    diagnostic.issue(),
                    diagnostic.message,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        panic!("Gradus consumer must emit Rust; diagnostics:\n{diagnostics}");
    };

    let generated = package.join("generated");
    let source = generated.join("src");
    fs::create_dir_all(&source).expect("create generated crate src");
    let runtime_path = runtime_crate_path();
    fs::write(
        generated.join("Cargo.toml"),
        format!(
            "[package]\nname = \"gradus-typed-consumer-generated\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfaber = {{ path = {:?}, default-features = false }}\n",
            runtime_path.display().to_string()
        ),
    )
    .expect("write generated crate manifest");
    fs::write(source.join("main.rs"), output.code).expect("write generated Rust");

    let target = generated.join("target");
    let run = generated_run(&generated, &target);
    if target.exists() {
        fs::remove_dir_all(&target).expect("remove generated Cargo target");
    }

    assert!(
        !run.timed_out,
        "generated Rust exceeded 120 seconds: {}",
        run.stderr
    );
    assert!(
        run.status.success(),
        "generated Rust failed ({:?}):\nstdout:\n{}\nstderr:\n{}",
        run.status,
        run.stdout,
        run.stderr
    );
    let mut lines = run.stdout.lines();
    let values = (0..4)
        .map(|_| {
            lines
                .next()
                .expect("generated program prints one Gradus add element per line")
                .trim()
                .parse::<f32>()
                .expect("parse generated tensor value")
        })
        .collect::<Vec<_>>();
    assert_eq!(values, vec![2.0, 5.0, 5.0, 8.0]);
    let error = lines
        .next()
        .expect("generated program prints the Gradus MSE result")
        .trim()
        .parse::<f32>()
        .expect("parse generated loss value");
    assert!((error - 7.5).abs() < f32::EPSILON);
    let flattened = lines
        .next()
        .expect("generated program prints the direct tensor flatten result")
        .trim()
        .parse::<f32>()
        .expect("parse generated flatten value");
    assert_eq!(flattened, 1.0);
    let product = lines
        .next()
        .expect("generated program prints the tensor dot result")
        .trim()
        .parse::<f32>()
        .expect("parse generated tensor dot value");
    assert_eq!(product, 7.0);
    assert_eq!(
        lines.map(str::trim).collect::<Vec<_>>(),
        vec!["3", "4", "0", "3"]
    );
}
