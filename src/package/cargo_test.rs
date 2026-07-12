use super::{
    coherent_runtime_cluster_root_from, local_repo_path_from, render_generated_cargo_toml,
    runtime_cluster_path_from, sibling_repo_path_from, RustRuntimePlan,
};
use crate::package::ManifestRustHost;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> Result<PathBuf, Box<dyn Error>> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(Box::<dyn Error>::from)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("faber-{label}-{nonce}"));
    fs::create_dir_all(&path)?;
    Ok(path)
}

#[test]
fn local_repo_path_prefers_nearest_worktree_sibling() -> Result<(), Box<dyn Error>> {
    let root = temp_dir("cargo-local-repo-path")?;
    let worktree = root.join("worktrees").join("slice").join("faber-build");
    fs::create_dir_all(&worktree)?;
    let worktree_parent = worktree
        .parent()
        .ok_or_else(|| std::io::Error::other("worktree path should have parent"))?;
    fs::create_dir_all(worktree_parent.join("faber-runtime"))?;
    fs::create_dir_all(root.join("faber-runtime"))?;

    assert_eq!(
        local_repo_path_from(&worktree, "faber-runtime"),
        worktree_parent.join("faber-runtime")
    );
    Ok(())
}

#[test]
fn sibling_repo_path_prefers_canonical_repo_cluster() -> Result<(), Box<dyn Error>> {
    let root = temp_dir("cargo-sibling-repo-path")?;
    let worktree = root.join("worktrees").join("slice").join("faber-build");
    fs::create_dir_all(&worktree)?;
    let cluster = worktree
        .parent()
        .ok_or_else(|| std::io::Error::other("worktree path should have parent"))?;
    for repo in ["faber-runtime", "host-kernel-rs", "host-native-rs"] {
        fs::create_dir_all(cluster.join(repo))?;
    }
    let direct = cluster.join("faber-runtime");
    let expected = fs::canonicalize(&direct).unwrap_or_else(|_| direct.clone());

    assert_eq!(sibling_repo_path_from(&worktree, "faber-runtime"), expected);
    Ok(())
}

#[test]
fn runtime_cluster_path_prefers_packet_root_layout_without_host_native(
) -> Result<(), Box<dyn Error>> {
    let root = temp_dir("cargo-packet-runtime-cluster")?;
    let packet = root.join("worktrees").join("slice");
    let package_root = packet.join("faber-build");
    fs::create_dir_all(&package_root)?;
    fs::write(packet.join("PACKET.md"), "# packet\n")?;
    fs::write(packet.join("MEMBERS.md"), "# members\n")?;
    fs::create_dir_all(packet.join("faber-runtime"))?;
    fs::create_dir_all(packet.join("host-kernel-rs"))?;
    fs::create_dir_all(
        packet
            .join("host-providers-rs")
            .join("crates")
            .join("sqlite"),
    )?;

    let canonical_root = root.join("canonical");
    fs::create_dir_all(canonical_root.join("faber-runtime"))?;
    fs::create_dir_all(canonical_root.join("host-kernel-rs"))?;
    fs::create_dir_all(canonical_root.join("host-native-rs"))?;
    fs::create_dir_all(
        canonical_root
            .join("host-providers-rs")
            .join("crates")
            .join("sqlite"),
    )?;

    let expected_runtime = fs::canonicalize(packet.join("faber-runtime"))
        .unwrap_or_else(|_| packet.join("faber-runtime"));
    let expected_kernel = fs::canonicalize(packet.join("host-kernel-rs"))
        .unwrap_or_else(|_| packet.join("host-kernel-rs"));
    let expected_provider = fs::canonicalize(packet.join("host-providers-rs"))
        .unwrap_or_else(|_| packet.join("host-providers-rs"));

    assert_eq!(
        runtime_cluster_path_from(&package_root, "faber-runtime"),
        expected_runtime
    );
    assert_eq!(
        runtime_cluster_path_from(&package_root, "host-kernel-rs"),
        expected_kernel
    );
    assert_eq!(
        runtime_cluster_path_from(&package_root, "host-providers-rs"),
        expected_provider
    );
    Ok(())
}

#[test]
fn native_runtime_plan_uses_one_packet_local_repo_cluster() -> Result<(), Box<dyn Error>> {
    let root = temp_dir("cargo-runtime-cluster")?;
    let workspace = root.join("workspace");
    let packet = workspace.join("worktrees").join("slice").join("pkg");
    let packet_parent = packet
        .parent()
        .ok_or_else(|| std::io::Error::other("packet path should have parent"))?;
    fs::create_dir_all(&packet)?;
    for repo in [
        "faber-runtime",
        "host-kernel-rs",
        "host-native-rs",
        "host-providers-rs",
    ] {
        fs::create_dir_all(packet_parent.join(repo))?;
    }
    fs::create_dir_all(
        packet_parent
            .join("host-providers-rs")
            .join("crates")
            .join("sqlite"),
    )?;

    let canonical_root = root.join("canonical");
    for repo in [
        "faber-runtime",
        "host-kernel-rs",
        "host-native-rs",
        "host-providers-rs",
    ] {
        fs::create_dir_all(canonical_root.join(repo))?;
    }

    let mut plan = RustRuntimePlan {
        needs_faber: true,
        host: Some(ManifestRustHost::Native),
        ..RustRuntimePlan::default()
    };
    plan.selected_providers.insert("sqlite".to_owned());

    let rendered = render_generated_cargo_toml("demo", "0.1.0", &plan, &packet);

    for expected in [
        packet_parent.join("faber-runtime"),
        packet_parent.join("host-kernel-rs"),
        packet_parent.join("host-native-rs"),
        packet_parent
            .join("host-providers-rs")
            .join("crates")
            .join("sqlite"),
    ] {
        let needle = expected.display().to_string();
        assert!(
            rendered.contains(&needle),
            "expected packet-local path {needle} in:\n{rendered}"
        );
    }
    assert!(
        !rendered.contains(&canonical_root.display().to_string()),
        "rendered cargo manifest should not pick the unrelated canonical cluster:\n{rendered}"
    );
    Ok(())
}

#[test]
fn native_runtime_plan_falls_back_to_one_canonical_cluster_when_packet_is_incomplete(
) -> Result<(), Box<dyn Error>> {
    let root = temp_dir("cargo-runtime-cluster-fallback")?;
    let packet = root.join("worktrees").join("slice").join("pkg");
    let packet_parent = packet
        .parent()
        .ok_or_else(|| std::io::Error::other("packet path should have parent"))?;
    fs::create_dir_all(&packet)?;
    fs::write(packet_parent.join("PACKET.md"), "# packet\n")?;
    fs::create_dir_all(packet_parent.join("faber-runtime"))?;
    fs::create_dir_all(packet_parent.join("host-native-rs"))?;

    let mut plan = RustRuntimePlan {
        needs_faber: true,
        host: Some(ManifestRustHost::Native),
        ..RustRuntimePlan::default()
    };
    plan.selected_providers.insert("sqlite".to_owned());

    let rendered = render_generated_cargo_toml("demo", "0.1.0", &plan, &packet);
    let expected_root = coherent_runtime_cluster_root_from(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
        &[
            "faber-runtime",
            "host-kernel-rs",
            "host-native-rs",
            "host-providers-rs",
        ],
    )
    .expect("repo runtime cluster root");

    for expected in [
        expected_root.join("faber-runtime"),
        expected_root.join("host-kernel-rs"),
        expected_root.join("host-native-rs"),
        expected_root
            .join("host-providers-rs")
            .join("crates")
            .join("sqlite"),
    ] {
        let needle = expected.display().to_string();
        assert!(
            rendered.contains(&needle),
            "expected canonical-cluster path {needle} in:\n{rendered}"
        );
    }
    assert!(
        !rendered.contains(&packet_parent.display().to_string()),
        "rendered cargo manifest should not mix incomplete packet-local cluster:\n{rendered}"
    );
    Ok(())
}

#[test]
fn generated_http_cargo_manifest_keeps_packet_runtime_pin_without_host_native(
) -> Result<(), Box<dyn Error>> {
    let root = temp_dir("cargo-http-runtime-pin")?;
    let packet = root.join("worktrees").join("slice");
    let package_root = packet.join("faber-build");
    fs::create_dir_all(&package_root)?;
    fs::write(packet.join("PACKET.md"), "# packet\n")?;
    fs::create_dir_all(packet.join("faber-runtime"))?;

    let canonical_root = root.join("canonical");
    fs::create_dir_all(canonical_root.join("faber-runtime"))?;
    fs::create_dir_all(canonical_root.join("host-kernel-rs"))?;
    fs::create_dir_all(canonical_root.join("host-native-rs"))?;

    let rendered = render_generated_cargo_toml(
        "demo-http",
        "0.1.0",
        &RustRuntimePlan {
            needs_faber: true,
            ..RustRuntimePlan::default()
        },
        &package_root,
    );

    let needle = packet.join("faber-runtime").display().to_string();
    assert!(
        rendered.contains(&needle),
        "expected packet-local runtime path {needle} in:\n{rendered}"
    );
    assert!(
        !rendered.contains(&canonical_root.display().to_string()),
        "rendered cargo manifest should not pick canonical main runtime:\n{rendered}"
    );
    Ok(())
}

#[test]
fn coherent_runtime_cluster_root_requires_all_requested_repos() -> Result<(), Box<dyn Error>> {
    let root = temp_dir("cargo-cluster-root-selection")?;
    let packet = root.join("worktrees").join("slice").join("pkg");
    let packet_parent = packet
        .parent()
        .ok_or_else(|| std::io::Error::other("packet path should have parent"))?;
    fs::create_dir_all(&packet)?;
    fs::write(packet_parent.join("PACKET.md"), "# packet\n")?;
    fs::create_dir_all(packet_parent.join("faber-runtime"))?;
    fs::create_dir_all(packet_parent.join("host-native-rs"))?;

    let canonical_root = root.join("canonical");
    for repo in [
        "faber-runtime",
        "host-kernel-rs",
        "host-native-rs",
        "host-providers-rs",
    ] {
        fs::create_dir_all(canonical_root.join(repo))?;
    }

    assert_eq!(
        coherent_runtime_cluster_root_from(&packet, &["faber-runtime", "host-native-rs"])
            .expect("packet runtime root"),
        fs::canonicalize(packet_parent).unwrap_or_else(|_| packet_parent.to_path_buf())
    );
    assert_eq!(
        coherent_runtime_cluster_root_from(
            &canonical_root,
            &[
                "faber-runtime",
                "host-kernel-rs",
                "host-native-rs",
                "host-providers-rs"
            ]
        )
        .expect("canonical complete root"),
        fs::canonicalize(&canonical_root).unwrap_or_else(|_| canonical_root.clone())
    );
    Ok(())
}

#[test]
fn generated_cargo_manifest_escapes_metadata_paths_and_dependency_keys(
) -> Result<(), Box<dyn Error>> {
    let version = "0.1.0\"\n# injected";
    let library_path = PathBuf::from("/tmp/library-\"-\\-path");
    let mut plan = RustRuntimePlan::default();
    plan.library_path_deps
        .push(("library\"key".to_owned(), library_path.clone()));

    let rendered =
        render_generated_cargo_toml("demo", version, &plan, PathBuf::from("/tmp").as_path());
    let manifest = toml::from_str::<toml::Value>(&rendered)?;
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or("missing package table")?;
    assert_eq!(
        package.get("version").and_then(toml::Value::as_str),
        Some(version)
    );
    let dependencies = manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .ok_or("missing dependency table")?;
    let dependency = dependencies
        .get("library\"key")
        .and_then(toml::Value::as_table)
        .ok_or("missing quoted dependency key")?;
    assert_eq!(
        dependency.get("path").and_then(toml::Value::as_str),
        Some(library_path.to_string_lossy().as_ref())
    );
    Ok(())
}
