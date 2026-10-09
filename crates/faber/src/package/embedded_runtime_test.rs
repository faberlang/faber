use super::*;
use crate::package::runtime_sources::is_support_runtime_root;

fn has(path: &str) -> bool {
    EMBEDDED_RUNTIME
        .iter()
        .any(|(relative, _)| *relative == path)
}

#[test]
fn table_carries_runtime_and_host_abi_manifests() {
    assert!(has("runtime/rust/Cargo.toml"));
    assert!(has("crates/radix-host-abi/Cargo.toml"));
    assert!(
        EMBEDDED_RUNTIME
            .iter()
            .all(|(relative, _)| !relative.contains("/target/")
                && !relative.contains("node_modules"))
    );
}

#[test]
fn embedded_root_materializes_once_and_matches_in_tree_sources() {
    let cache = tempfile::tempdir().expect("tempdir");
    // SAFETY: the only test that sets this variable; it names its own dir.
    unsafe { std::env::set_var(CACHE_HOME_ENV, cache.path()) };
    let root = embedded_runtime_root().expect("embedded root");
    assert!(is_support_runtime_root(&root));
    let in_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/rust/Cargo.toml");
    assert_eq!(
        fs::read(root.join("runtime/rust/Cargo.toml")).expect("extracted"),
        fs::read(in_tree).expect("in-tree"),
    );
    assert!(root.join("crates/radix-host-abi/Cargo.toml").is_file());

    let marker = root.join(COMPLETE_MARKER);
    let before = fs::metadata(&marker)
        .and_then(|m| m.modified())
        .expect("mtime");
    let again = embedded_runtime_root().expect("second call");
    assert_eq!(again, root);
    let after = fs::metadata(&marker)
        .and_then(|m| m.modified())
        .expect("mtime");
    assert_eq!(before, after, "second call must not rewrite the extraction");
}
