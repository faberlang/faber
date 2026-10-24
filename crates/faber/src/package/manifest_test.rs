//! Tests for the `[device] device` manifest key (differentiable-GPU campaign
//! S1-5, N1.1): parsing, fail-closed validation, and the precedence fold; and
//! for the `[device] threads` CPU worker count (gpu-reset rule 16.9).

use super::*;
use radix::module::diagnostics::DiagnosticArg;
use std::path::Path;

#[test]
fn absent_device_key_is_none() {
    let result = manifest_device_choice(None, Path::new("faber.toml")).expect("absent key");
    assert_eq!(result, None);
}

#[test]
fn valid_device_spellings_parse() {
    for (spelling, expected) in [
        ("auto", DeviceChoice::Auto),
        ("reference", DeviceChoice::Reference),
        ("cpu", DeviceChoice::Cpu),
        ("metal", DeviceChoice::Metal),
        ("cuda", DeviceChoice::Cuda),
    ] {
        let result =
            manifest_device_choice(Some(spelling), Path::new("faber.toml")).expect("parses");
        assert_eq!(result, Some(expected), "spelling {spelling}");
    }
}

#[test]
fn whitespace_around_spelling_is_tolerated() {
    let result = manifest_device_choice(Some("  metal  "), Path::new("faber.toml"))
        .expect("trimmed spelling");
    assert_eq!(result, Some(DeviceChoice::Metal));
}

#[test]
fn unsupported_device_fails_closed_with_issue() {
    let err = manifest_device_choice(Some("rocm"), Path::new("faber.toml"))
        .expect_err("unsupported value must fail closed");
    assert_eq!(err.issue(), Some("package_device_unsupported"));
    assert!(err.args.contains(&DiagnosticArg::new("device", "rocm")));
}

fn manifest_with_target_tables(tables: &str) -> FaberManifest {
    toml::from_str(&format!(
        "[package]\nname = \"demo\"\n\n[paths]\nentry = \"main.fab\"\n{tables}"
    ))
    .expect("manifest parses")
}

#[test]
fn validate_manifest_rejects_unknown_target_table() {
    let manifest = manifest_with_target_tables("\n[target.wgsl]\nbindings = \"b.toml\"\n");
    let err = validate_manifest(&manifest, Path::new("faber.toml"))
        .expect_err("an unknown [target.<key>] must fail check/build validation");
    assert_eq!(err.issue(), Some("package_target_table_unknown"));
}

#[test]
fn validate_manifest_accepts_target_spelling_table() {
    let manifest = manifest_with_target_tables("\n[target.wgsl-text]\nbindings = \"b.toml\"\n");
    validate_manifest(&manifest, Path::new("faber.toml"))
        .expect("a target-spelling table passes validation");
}

// ── `[device] threads` worker count (gpu-reset rule 16.9) ────────────────

fn device_table(body: &str) -> ManifestDevice {
    toml::from_str(body).expect("[device] table parses")
}

#[test]
fn absent_threads_key_is_none() {
    let device = device_table("device = \"cpu\"\n");
    assert_eq!(
        manifest_device_threads(&device, Path::new("faber.toml")).expect("absent key"),
        None
    );
}

#[test]
fn positive_threads_parse_beside_cpu_or_absent_device() {
    for body in ["threads = 4\n", "device = \"cpu\"\nthreads = 4\n"] {
        let count = manifest_device_threads(&device_table(body), Path::new("faber.toml"))
            .expect("a positive count parses")
            .expect("count present");
        assert_eq!(count.get(), 4, "{body}");
    }
}

#[test]
fn invalid_threads_fail_closed_with_issue() {
    for (body, shown) in [
        ("threads = 0\n", "0"),
        ("threads = -2\n", "-2"),
        ("threads = 2.5\n", "2.5"),
        ("threads = \"two\"\n", "\"two\""),
        ("threads = 4294967296\n", "4294967296"),
    ] {
        let err = manifest_device_threads(&device_table(body), Path::new("faber.toml"))
            .expect_err("an invalid count must fail closed");
        assert_eq!(
            err.issue(),
            Some("package_device_threads_invalid"),
            "{body}"
        );
        assert!(
            err.args
                .contains(&DiagnosticArg::new("key", "device.threads")),
            "{body}"
        );
        assert!(
            err.args.contains(&DiagnosticArg::new("threads", shown)),
            "{body}"
        );
        assert!(err.message.contains("positive integer"), "{}", err.message);
    }
}

#[test]
fn threads_beside_a_non_cpu_device_fail_closed() {
    for device in ["auto", "reference", "metal", "cuda"] {
        let body = format!("device = \"{device}\"\nthreads = 2\n");
        let err = manifest_device_threads(&device_table(&body), Path::new("faber.toml"))
            .expect_err("threads with a non-cpu device must fail closed");
        assert_eq!(
            err.issue(),
            Some("package_device_threads_requires_cpu"),
            "{device}"
        );
        assert!(err.args.contains(&DiagnosticArg::new("device", device)));
    }
}

#[test]
fn validate_manifest_rejects_zero_threads() {
    let manifest = manifest_with_target_tables("\n[device]\nthreads = 0\n");
    let err = validate_manifest(&manifest, Path::new("faber.toml"))
        .expect_err("a zero worker count must fail package validation");
    assert_eq!(err.issue(), Some("package_device_threads_invalid"));
}

/// A package directory whose manifest carries `device_lines` under `[device]`.
fn threads_package(label: &str, device_lines: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("faber-manifest-threads-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    std::fs::write(dir.join("main.fab"), "main {\n}\n").expect("write entry");
    std::fs::write(
        dir.join("faber.toml"),
        format!(
            "[package]\nname = \"threads-precedence\"\nversion = \"0.1.0\"\nedition = \"1\"\n\n[paths]\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n\n[device]\n{device_lines}"
        ),
    )
    .expect("write manifest");
    dir
}

#[test]
fn cpu_threads_precedence_flag_then_manifest_then_default() {
    let with_key = threads_package("with-key", "threads = 4\n");
    let resolve = |cli, dir: &Path| {
        resolve_cpu_threads(cli, DeviceSelection::Cpu, dir)
            .expect("resolves")
            .map(std::num::NonZeroU32::get)
    };
    assert_eq!(resolve(Some(2), &with_key), Some(2), "the flag wins");
    assert_eq!(resolve(None, &with_key), Some(4), "the manifest applies");
    let without_key = threads_package("without-key", "");
    assert_eq!(resolve(None, &without_key), None, "the device default");
}

#[test]
fn manifest_threads_apply_only_on_the_cpu_device() {
    let dir = threads_package("reference", "threads = 4\n");
    for selection in [DeviceSelection::Reference, DeviceSelection::Auto] {
        assert_eq!(
            resolve_cpu_threads(None, selection, &dir).expect("resolves"),
            None,
            "{selection:?}"
        );
    }
}

#[test]
fn threads_flag_requires_the_cpu_device() {
    let dir = threads_package("flag-reference", "");
    let err = resolve_cpu_threads(Some(2), DeviceSelection::Reference, &dir)
        .expect_err("--threads with reference must fail closed");
    assert_eq!(err.issue(), Some("E_THREADS_REQUIRES_CPU"));
    assert!(
        err.args
            .contains(&DiagnosticArg::new("device", "reference"))
    );
}

#[test]
fn invalid_manifest_threads_fail_the_cpu_route() {
    let dir = threads_package("invalid", "threads = -1\n");
    let err = resolve_cpu_threads(None, DeviceSelection::Cpu, &dir)
        .expect_err("an invalid manifest count fails the cpu route");
    assert_eq!(err.issue(), Some("package_device_threads_invalid"));
}
