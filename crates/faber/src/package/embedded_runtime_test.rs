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
    // An explicit cache directory, not the process-wide env var: other tests
    // resolve the runtime concurrently and must not see this temp dir.
    let root = embedded_runtime_root_in(cache.path()).expect("embedded root");
    assert!(is_support_runtime_root(&root));
    let in_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/rust/Cargo.toml");
    assert_eq!(
        fs::read(root.join("runtime/rust/Cargo.toml")).expect("extracted"),
        fs::read(in_tree).expect("in-tree"),
    );
    assert!(root.join("crates/radix-host-abi/Cargo.toml").is_file());

    // Norma travels with the binary: a relocated root (a temp cache, no
    // checkout) carries the source, locale packs and manifests the library
    // resolver reads, byte-identical to the build tree.
    assert!(root.starts_with(cache.path()));
    for norma in [
        "src/fs.fab",
        "faber.toml",
        "cista.toml",
        "locale/en/pack.toml",
    ] {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../lib/norma")
            .join(norma);
        assert_eq!(
            fs::read(root.join("lib/norma").join(norma)).expect("extracted norma"),
            fs::read(source).expect("in-tree norma"),
            "lib/norma/{norma}"
        );
    }

    let marker = root.join(COMPLETE_MARKER);
    let before = fs::metadata(&marker)
        .and_then(|m| m.modified())
        .expect("mtime");
    let again = embedded_runtime_root_in(cache.path()).expect("second call");
    assert_eq!(again, root);
    let after = fs::metadata(&marker)
        .and_then(|m| m.modified())
        .expect("mtime");
    assert_eq!(before, after, "second call must not rewrite the extraction");
}
