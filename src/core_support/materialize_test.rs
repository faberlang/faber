use super::*;
use std::sync::Arc;
use std::thread;

fn test_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().join(format!(
        "faber-materialize-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    root
}

fn cleanup(root: &Path) {
    #[cfg(unix)]
    fn make_writable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if metadata.is_dir() {
                for child in fs::read_dir(path).into_iter().flatten().flatten() {
                    make_writable(&child.path());
                }
            }
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
        }
    }
    #[cfg(unix)]
    make_writable(root);
    let _ = fs::remove_dir_all(root);
}

fn payload(entries: &[(&str, &[u8])]) -> (Vec<u8>, String, String) {
    let mut tar = tar::Builder::new(Vec::new());
    let mut manifest = String::new();
    for (name, data) in entries {
        let mut header = tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, name, *data).unwrap();
        manifest.push_str(&format!("{}  {name}\n", hex(&Sha256::digest(*data))));
    }
    let archive = zstd::stream::encode_all(tar.into_inner().unwrap().as_slice(), 19).unwrap();
    let hash = hex(&Sha256::digest(&archive));
    (archive, hash, manifest)
}

fn symlink_payload() -> (Vec<u8>, String, String) {
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::symlink());
    header.set_size(0);
    header.set_mode(0o777);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_path("faber-runtime/src/lib.rs").unwrap();
    header.set_link_name("../../escape").unwrap();
    header.set_cksum();
    tar.append(&header, io::empty()).unwrap();
    let archive = zstd::stream::encode_all(tar.into_inner().unwrap().as_slice(), 19).unwrap();
    let hash = hex(&Sha256::digest(&archive));
    let manifest = format!(
        "{}  faber-runtime/src/lib.rs\n",
        hex(&Sha256::digest(b"ok"))
    );
    (archive, hash, manifest)
}

#[test]
fn materializes_verified_files_and_reuses_a_completed_entry() {
    let root = test_root("reuse");
    let (archive, hash, manifest) =
        payload(&[("faber-runtime/src/lib.rs", b"pub fn runtime() {}")]);
    let first = materialize_payload(&root, &archive, &hash, &manifest).unwrap();
    assert_eq!(
        fs::read(first.root().join("faber-runtime/src/lib.rs")).unwrap(),
        b"pub fn runtime() {}"
    );
    assert!(first.root().join(COMPLETION_FILE).is_file());
    let second = materialize_payload(&root, &archive, &hash, &manifest).unwrap();
    assert_eq!(first, second);
    cleanup(&root);
}

#[test]
fn rejects_hash_mismatch_unexpected_duplicate_and_unsafe_paths() {
    let root = test_root("reject");
    let (archive, _hash, manifest) = payload(&[("faber-runtime/src/lib.rs", b"ok")]);
    assert!(materialize_payload(&root, &archive, &"0".repeat(64), &manifest).is_err());
    let (unexpected, unexpected_hash, _) = payload(&[
        ("faber-runtime/src/lib.rs", b"ok"),
        ("unexpected.rs", b"no"),
    ]);
    assert!(materialize_payload(&root, &unexpected, &unexpected_hash, &manifest).is_err());
    let (duplicate, duplicate_hash, _) = payload(&[
        ("faber-runtime/src/lib.rs", b"ok"),
        ("faber-runtime/src/lib.rs", b"ok"),
    ]);
    assert!(materialize_payload(&root, &duplicate, &duplicate_hash, &manifest).is_err());
    assert!(parse_manifest(&format!("{}  ../escape\n", "0".repeat(64))).is_err());
    assert!(!is_safe_relative(Path::new("../escape")));
    let (symlink, symlink_hash, symlink_manifest) = symlink_payload();
    assert!(materialize_payload(&root, &symlink, &symlink_hash, &symlink_manifest).is_err());
    cleanup(&root);
}

#[test]
fn rejects_oversized_entries() {
    let root = test_root("oversize");
    let contents = vec![b'x'; (MAX_FILE_BYTES + 1) as usize];
    let (archive, hash, manifest) = payload(&[("faber-runtime/src/lib.rs", &contents)]);
    assert!(materialize_payload(&root, &archive, &hash, &manifest).is_err());
    cleanup(&root);
}

#[test]
fn concurrent_materializations_converge_on_one_completed_entry() {
    let root = Arc::new(test_root("concurrent"));
    let (archive, hash, manifest) = payload(&[("faber-runtime/src/lib.rs", b"ok")]);
    let archive = Arc::new(archive);
    let manifest = Arc::new(manifest);
    let mut workers = Vec::new();
    for _ in 0..8 {
        let root = Arc::clone(&root);
        let archive = Arc::clone(&archive);
        let manifest = Arc::clone(&manifest);
        let hash = hash.clone();
        workers.push(thread::spawn(move || {
            materialize_payload(&root, &archive, &hash, &manifest).unwrap()
        }));
    }
    let paths = workers
        .into_iter()
        .map(|worker| worker.join().unwrap().root().to_path_buf())
        .collect::<BTreeSet<_>>();
    assert_eq!(paths.len(), 1);
    cleanup(root.as_ref());
}

#[cfg(unix)]
#[test]
fn rejects_a_symlinked_cache_entry() {
    use std::os::unix::fs::symlink;

    let root = test_root("symlink");
    let (archive, hash, manifest) = payload(&[("faber-runtime/src/lib.rs", b"ok")]);
    let parent = root.join("faber/core-support").join(PAYLOAD_FORMAT);
    fs::create_dir_all(&parent).unwrap();
    let outside = root.join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, parent.join(&hash)).unwrap();
    assert!(materialize_payload(&root, &archive, &hash, &manifest).is_err());
    cleanup(&root);
}

#[test]
fn repairs_noncompleted_state_but_preserves_completed_corruption() {
    let root = test_root("recovery");
    let (archive, hash, manifest) = payload(&[("faber-runtime/src/lib.rs", b"ok")]);
    let target = root
        .join("faber/core-support")
        .join(PAYLOAD_FORMAT)
        .join(&hash);
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("partial"), "partial").unwrap();
    let materialized = materialize_payload(&root, &archive, &hash, &manifest).unwrap();
    assert!(materialized.root().join(COMPLETION_FILE).exists());
    let parent = materialized.root().parent().unwrap();
    assert!(fs::read_dir(parent)
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().contains(".incomplete-")));

    let file = materialized.root().join("faber-runtime/src/lib.rs");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    }
    fs::write(&file, "corrupt").unwrap();
    assert!(materialize_payload(&root, &archive, &hash, &manifest).is_err());
    assert!(materialized.root().exists());
    cleanup(&root);
}
