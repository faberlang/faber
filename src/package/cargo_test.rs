use super::{
    local_repo_path_from, render_generated_cargo_toml, sibling_repo_path_from, RustRuntimePlan,
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
