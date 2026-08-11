//! Unit tests for `faber self update` planning (prefix discovery, receipt
//! parsing, asset-base derivation, engine invocation shape). The update engine
//! itself (checksum-before-exec, lanes, rollback) is proven by
//! `scripta/install-faber-test.py`; this file covers the wrapper's pure logic.

use super::self_update::*;

fn args(version: &str) -> SelfUpdateArgs {
    SelfUpdateArgs {
        version: version.to_owned(),
        prefix: None,
        base_url: None,
        allow_lane_change: false,
    }
}

fn write_receipt(prefix: &Path, data: &serde_json::Value) {
    let path = prefix.join(RECEIPT_REL);
    std::fs::create_dir_all(path.parent().expect("receipt parent")).expect("mkdir");
    std::fs::write(path, serde_json::to_vec(data).expect("receipt json")).expect("write");
}

fn receipt_json(source: &str, version: &str) -> serde_json::Value {
    serde_json::json!({
        "receipt": "faber install receipt",
        "prefix": "/tmp/x",
        "version": version,
        "triple": "aarch64-apple-darwin",
        "archive": format!("faber-v{version}-aarch64-apple-darwin.tar.gz"),
        "archiveSha256": "ab" .repeat(32),
        "source": source,
        "installedAt": "2026-08-11T00:00:00Z",
        "files": ["bin/faber"],
    })
}

#[test]
fn plan_requires_version() {
    let tmp = tempfile::tempdir().expect("tmp");
    let exe = tmp.path().join("bin/faber");
    let err = plan_self_update(&args(""), Some(&exe), None).expect_err("empty version must fail");
    assert!(err.contains("--version is required"), "{err}");
}

#[test]
fn plan_reads_receipt_relative_to_binary() {
    let tmp = tempfile::tempdir().expect("tmp");
    let prefix = tmp.path().join("prefix");
    let source = format!(
        "https://github.com/faberlang/releases/releases/download/faber-v1.5.0/faber-v1.5.0-aarch64-apple-darwin.tar.gz"
    );
    write_receipt(&prefix, &receipt_json(&source, "1.5.0"));
    let exe = prefix.join("bin/faber");
    let plan = plan_self_update(&args("1.6.0"), Some(&exe), None).expect("plan");
    assert_eq!(plan.prefix, prefix);
    assert_eq!(plan.current_version, "1.5.0");
    assert_eq!(plan.target_version, "1.6.0");
    assert_eq!(plan.triple, "aarch64-apple-darwin");
    // assetBase is absent in this receipt -> derive from the release-host source
    assert_eq!(
        plan.base_url,
        "https://github.com/faberlang/releases/releases/download/faber-v1.6.0"
    );
    assert_eq!(
        plan.script_url,
        "https://github.com/faberlang/releases/releases/download/faber-v1.6.0/install-faber"
    );
    assert!(!plan.allow_lane_change);
}

#[test]
fn plan_prefers_recorded_asset_base() {
    let tmp = tempfile::tempdir().expect("tmp");
    let prefix = tmp.path().join("prefix");
    let mut receipt = receipt_json(
        "https://host.example/download/faber-v1.5.0/faber-v1.5.0-aarch64-apple-darwin.tar.gz",
        "1.5.0",
    );
    receipt["assetBase"] = serde_json::json!("https://mirror.example/faber");
    write_receipt(&prefix, &receipt);
    let plan = plan_self_update(&args("1.6.0"), Some(&prefix.join("bin/faber")), None)
        .expect("plan");
    assert_eq!(plan.base_url, "https://mirror.example/faber");
    assert_eq!(plan.script_url, "https://mirror.example/faber/install-faber");
}

#[test]
fn plan_base_url_override_wins() {
    let tmp = tempfile::tempdir().expect("tmp");
    let prefix = tmp.path().join("prefix");
    write_receipt(
        &prefix,
        &receipt_json("https://host.example/download/faber-v1.5.0/faber-v1.5.0-aarch64-apple-darwin.tar.gz", "1.5.0"),
    );
    let mut a = args("1.6.0");
    a.base_url = Some("/tmp/local-mirror".to_owned());
    let plan = plan_self_update(&a, Some(&prefix.join("bin/faber")), None).expect("plan");
    assert_eq!(plan.base_url, "/tmp/local-mirror");
    assert_eq!(plan.script_url, "/tmp/local-mirror/install-faber");
}

#[test]
fn plan_missing_receipt_fails_closed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let exe = tmp.path().join("bin/faber");
    let err = plan_self_update(&args("1.6.0"), Some(&exe), None).expect_err("no receipt");
    assert!(err.contains("no faber install found"), "{err}");
}

#[test]
fn plan_missing_version_field_fails_closed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let prefix = tmp.path().join("prefix");
    write_receipt(&prefix, &serde_json::json!({"prefix": "/tmp/x"}));
    let err =
        plan_self_update(&args("1.6.0"), Some(&prefix.join("bin/faber")), None).expect_err("bad");
    assert!(err.contains("no version"), "{err}");
}

#[test]
fn explicit_prefix_is_used() {
    let tmp = tempfile::tempdir().expect("tmp");
    let prefix = tmp.path().join("prefix");
    write_receipt(&prefix, &receipt_json("/tmp/local-mirror/faber-v1.5.0-aarch64-apple-darwin.tar.gz", "1.5.0"));
    let mut a = args("1.6.0");
    a.prefix = Some(prefix.clone());
    let plan = plan_self_update(&a, None, None).expect("plan");
    assert_eq!(plan.prefix, prefix);
}

#[test]
fn allow_lane_change_is_forwarded() {
    let tmp = tempfile::tempdir().expect("tmp");
    let prefix = tmp.path().join("prefix");
    write_receipt(&prefix, &receipt_json("/tmp/m/faber-v1.5.0-aarch64-apple-darwin.tar.gz", "1.5.0"));
    let mut a = args("2.0.0");
    a.allow_lane_change = true;
    let plan = plan_self_update(&a, Some(&prefix.join("bin/faber")), None).expect("plan");
    assert!(plan.allow_lane_change);
}

#[test]
fn derive_base_url_replaces_release_tag() {
    assert_eq!(
        derive_base_url(
            "https://github.com/faberlang/releases/releases/download/faber-v1.5.0/faber-v1.5.0-aarch64-apple-darwin.tar.gz",
            "1.6.0",
        ),
        "https://github.com/faberlang/releases/releases/download/faber-v1.6.0"
    );
}

#[test]
fn derive_base_url_keeps_flat_local_dir() {
    assert_eq!(
        derive_base_url(
            "/tmp/release-host/faber-v1.5.0-aarch64-apple-darwin.tar.gz",
            "1.6.0",
        ),
        "/tmp/release-host"
    );
}

#[test]
fn derive_base_url_keeps_tag_shaped_local_dir() {
    assert_eq!(
        derive_base_url(
            "/tmp/release-host/faber-v1.5.0/faber-v1.5.0-aarch64-apple-darwin.tar.gz",
            "1.6.0",
        ),
        "/tmp/release-host/faber-v1.6.0"
    );
}

#[test]
fn cli_shape_has_update_subcommand() {
    // The `faber self` surface exposes exactly the update subcommand for now;
    // dispatch requires it (A5 adds uninstall later).
    let command = SelfCommand::Update(args("1.6.0"));
    let manage = SelfManageArgs { command };
    match manage.command {
        SelfCommand::Update(inner) => assert_eq!(inner.version, "1.6.0"),
    }
}
