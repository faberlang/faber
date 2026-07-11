use super::{local_repo_path_from, sibling_repo_path_from};
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
