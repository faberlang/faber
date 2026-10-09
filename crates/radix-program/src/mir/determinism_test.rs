//! Build-twice determinism sweep (register row F-35).
//!
//! The same package must lower to byte-identical MIR artifacts every time.
//! std `HashMap`/`HashSet` seed per instance and per process, so the in-process
//! comparison is only a smoke test: the real proof re-runs the build in fresh
//! processes (the test binary re-executes itself), where every map draws a new
//! seed, and compares the artifact bytes.
//!
//! Corpus: one package with a sibling module (class, union), the gradus
//! `math`/`loss` libraries (size-generic kernels, tensor carriers, library
//! nominals) and several local generics. Artifacts: the package MIR text dump,
//! the fmir-text image (types, interner, program JSON, sources), and the
//! binary fmir image.

use super::*;
use std::process::Command;

const CHILD_PACKAGE_ENV: &str = "RADIX_DETERMINISM_PACKAGE";
const CHILD_OUT_ENV: &str = "RADIX_DETERMINISM_OUT";
const CHILD_TEST: &str = "mir::determinism_test::child_builds_artifacts";
const CROSS_PROCESS_RUNS: usize = 3;

const UTIL: &str = r#"@ public
class Persona {
    var string nomen
}

@ public
fn saluta(Persona p) → string {
    return p.nomen
}

@ public
union Color {
    rubrum,
    caeruleum
}

@ public
fn primus() → Color {
    return Color.rubrum
}

@ public
fn colore_nomen(Color c) → string {
    match c {
        case rubrum {
            return "rubrum"
        }
        case caeruleum {
            return "caeruleum"
        }
    }
}
"#;

const OTHER: &str = r#"@ public
class Persona {
    var string titulus
}

@ public
fn other_titulus(Persona p) → string {
    return p.titulus
}
"#;

const MAIN: &str = r#"+++
locale = "en"
+++

import from "./util" * as utilModule
import from "./other" * as otherModule
import from "gradus:math" math
import from "gradus:loss" loss

fn drop_one<size N>(tensor<f32, [N - 1]> value) → tensor<f32, [N - 1]> { return value }
fn half_width<size D>(tensor<f32, [D / 2]> value) → tensor<f32, [D / 2]> { return value }
fn pass_through<size M, size N>(tensor<f32, [M, N]> value) → tensor<f32, [M, N]> { return value }

fn describe() → string {
    const utilModule.Persona una ← utilModule.Persona { nomen = "salve" }
    const otherModule.Persona altera ← otherModule.Persona { titulus = "hei" }
    return utilModule.saluta(una) + "-" + otherModule.other_titulus(altera) + "-" + utilModule.colore_nomen(utilModule.primus())
}

main {
    const tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    const tensor<f32, [2, 2]> b ← [[10.0, 20.0], [30.0, 40.0]] ↦ tensor<f32, [2, 2]>
    const tensor<f32, [2, 2]> sum ← math.add(pass_through(a), b)
    const f32 error ← loss.mse(sum, b)
    print error
    const list<f32> three_values ← [1.0, 2.0, 3.0]
    const tensor<f32, [3]> three ← three_values ↦ tensor<f32, [3]>
    const tensor<f32, [2]> tail ← drop_one(three)
    print tail.shape()[0]
    const list<f32> four_values ← [1.0, 2.0, 3.0, 4.0]
    const tensor<f32, [4]> four ← four_values ↦ tensor<f32, [4]>
    const tensor<f32, [2]> half ← half_width(four)
    print half.shape()[0]
    print describe()
}
"#;

fn write_package(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        "[package]\nname = \"determinism-sweep\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n\n[locale]\nlocale = \"en\"\n",
    )
    .expect("write faber.toml");
    fs::write(src.join("util.fab"), UTIL).expect("write util.fab");
    fs::write(src.join("other.fab"), OTHER).expect("write other.fab");
    fs::write(src.join("main.fab"), MAIN).expect("write main.fab");
}

fn config() -> Config {
    let library_home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("resolve packet library home");
    Config::default().with_library_home(library_home)
}

fn describe_failure(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| format!("{diagnostic:?}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build the corpus once and return every text/binary artifact, labelled.
fn build_artifacts(package: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let config = config();
    let seams = crate::test_support::test_mir_seams();
    let entry = package.join("src/main.fab");

    let dump = with_interpreted_lowered_package_mir(&config, &entry, &seams, |lowered| {
        radix_module::mir::dump_program(lowered.validated.program())
    })
    .unwrap_or_else(|diagnostics| panic!("lower corpus: {}", describe_failure(&diagnostics)));

    let text = build_package_fmir_text_image(&config, package, &[], &seams)
        .unwrap_or_else(|diagnostics| panic!("fmir-text: {}", describe_failure(&diagnostics)));
    let binary = build_package_fmir_image(&config, package, &[], &seams)
        .unwrap_or_else(|diagnostics| panic!("fmir: {}", describe_failure(&diagnostics)));

    vec![
        ("mir-dump", dump.into_bytes()),
        (
            "fmir-text",
            fs::read(&text.image_path).expect("read fmir-text image"),
        ),
        (
            "fmir",
            fs::read(&binary.image_path).expect("read fmir image"),
        ),
    ]
}

fn first_difference(left: &[u8], right: &[u8]) -> String {
    let at = left
        .iter()
        .zip(right)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| left.len().min(right.len()));
    let window = |bytes: &[u8]| {
        let start = at.saturating_sub(120);
        let end = (at + 120).min(bytes.len());
        String::from_utf8_lossy(&bytes[start..end]).into_owned()
    };
    format!(
        "first difference at byte {at} (lengths {} vs {})\n--- left ---\n{}\n--- right ---\n{}",
        left.len(),
        right.len(),
        window(left),
        window(right),
    )
}

fn assert_same(
    label: &str,
    run: &str,
    base: &[(&'static str, Vec<u8>)],
    other: &[(&str, Vec<u8>)],
) {
    assert_eq!(base.len(), other.len(), "{label}: artifact count ({run})");
    for ((name, left), (_, right)) in base.iter().zip(other) {
        assert!(
            left == right,
            "{label}: artifact `{name}` differs ({run})\n{}",
            first_difference(left, right)
        );
    }
}

/// Serialize labelled artifacts as `name\nlength\nbytes` records.
fn encode(artifacts: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, bytes) in artifacts {
        out.extend_from_slice(format!("{name}\n{}\n", bytes.len()).as_bytes());
        out.extend_from_slice(bytes);
    }
    out
}

fn decode(mut bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut artifacts = Vec::new();
    let line = |bytes: &mut &[u8]| {
        let end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("record header line");
        let (head, rest) = bytes.split_at(end);
        *bytes = &rest[1..];
        String::from_utf8(head.to_vec()).expect("utf-8 record header")
    };
    while !bytes.is_empty() {
        let name = line(&mut bytes);
        let length: usize = line(&mut bytes).parse().expect("record length");
        let (body, rest) = bytes.split_at(length);
        artifacts.push((name, body.to_vec()));
        bytes = rest;
    }
    artifacts
}

/// Child half of the cross-process proof: a no-op unless the parent test
/// re-executes this binary with the package and output paths in the
/// environment. Each child process draws fresh std `HashMap` seeds.
#[test]
fn child_builds_artifacts() {
    let (Some(package), Some(out)) = (
        std::env::var_os(CHILD_PACKAGE_ENV),
        std::env::var_os(CHILD_OUT_ENV),
    ) else {
        return;
    };
    let artifacts = build_artifacts(Path::new(&package));
    fs::write(out, encode(&artifacts)).expect("write child artifacts");
}

#[test]
fn package_mir_artifacts_are_identical_across_builds_and_processes() {
    let dir = crate::test_support::test_temp_dir("determinism-sweep");
    write_package(dir.as_ref());

    // In-process: the same package built twice in this process.
    let first = build_artifacts(dir.as_ref());
    let second = build_artifacts(dir.as_ref());
    assert_same("in-process", "second build", &first, &second);

    // Cross-process: fresh processes, fresh per-process hash seeds.
    let exe = std::env::current_exe().expect("test binary path");
    for run in 0..CROSS_PROCESS_RUNS {
        let out = dir.join(format!("child-{run}.bin"));
        let output = Command::new(&exe)
            .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD_PACKAGE_ENV, dir.path())
            .env(CHILD_OUT_ENV, &out)
            .output()
            .expect("spawn determinism child");
        assert!(
            output.status.success(),
            "child {run} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let child = decode(&fs::read(&out).expect("read child artifacts"));
        let child: Vec<(&str, Vec<u8>)> = child
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.clone()))
            .collect();
        assert_same(
            "cross-process",
            &format!("child process {run}"),
            &first,
            &child,
        );
    }
}
