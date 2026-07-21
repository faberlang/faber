#[path = "core_support/assembler.rs"]
mod assembler;

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "faber-core-support-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for entry in assembler::EXPECTED_ROOTS {
        let path = root.join(entry);
        if Path::new(entry).extension().is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "[package]\nname = \"fixture\"\n").unwrap();
        } else {
            fs::create_dir_all(path.join("src")).unwrap();
            fs::write(path.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
            fs::write(path.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
        }
    }
    root
}

#[test]
fn assembly_is_deterministic_and_uses_only_the_explicit_roots() {
    let root = fixture();
    fs::create_dir_all(root.join("hosts/crates/unlisted/src")).unwrap();
    fs::write(
        root.join("hosts/crates/unlisted/src/lib.rs"),
        "forbidden",
    )
    .unwrap();
    fs::create_dir_all(root.join("hosts/crates/host-kernel/target")).unwrap();
    fs::write(
        root.join("hosts/crates/host-kernel/target/ignored"),
        "binary cache",
    )
    .unwrap();
    fs::create_dir_all(root.join("hosts/crates/host-kernel/build")).unwrap();
    fs::write(
        root.join("hosts/crates/host-kernel/build/ignored"),
        "generated cache",
    )
    .unwrap();
    fs::create_dir_all(root.join("hosts/crates/host-kernel/cache")).unwrap();
    fs::write(
        root.join("hosts/crates/host-kernel/cache/ignored"),
        "cache state",
    )
    .unwrap();

    let roots = assembler::EXPECTED_ROOTS
        .iter()
        .map(|root| (*root).to_owned())
        .collect::<Vec<_>>();
    let first = assembler::assemble(&root, &roots).unwrap();
    let second = assembler::assemble(&root, &roots).unwrap();
    assert_eq!(first, second);
    assert!(first.files.iter().all(|file| {
        !file.path.contains("unlisted")
            && !file.path.contains("/target/")
            && !file.path.contains("/build/")
            && !file.path.contains("/cache/")
    }));
    assert!(first
        .files
        .iter()
        .any(|file| file.path == "hosts/crates/aleator/src/lib.rs"));
    assert!(assembler::file_manifest(&first.files).contains("hosts/crates/aleator/src/lib.rs"));

    let mut archive =
        tar::Archive::new(zstd::stream::read::Decoder::new(first.archive.as_slice()).unwrap());
    let names = archive
        .entries()
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .path()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert!(names.iter().all(|name| {
        !name.contains("unlisted")
            && !name.contains("/target/")
            && !name.contains("/build/")
            && !name.contains("/cache/")
    }));
    assert_eq!(
        names,
        first
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn embedded_archive_matches_its_build_metadata() {
    assert_eq!(
        format!("{:x}", Sha256::digest(super::core_support::ARCHIVE)),
        super::core_support::SHA256
    );
    let mut archive =
        tar::Archive::new(zstd::stream::read::Decoder::new(super::core_support::ARCHIVE).unwrap());
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        let header = entry.header();
        assert_eq!(header.uid().unwrap(), 0);
        assert_eq!(header.gid().unwrap(), 0);
        assert_eq!(header.mtime().unwrap(), 0);
        assert_eq!(header.mode().unwrap(), 0o644);
    }
}

#[test]
fn secret_like_and_executable_files_fail_closed() {
    let root = fixture();
    let roots = assembler::EXPECTED_ROOTS
        .iter()
        .map(|root| (*root).to_owned())
        .collect::<Vec<_>>();
    let secret = root.join("hosts/crates/host-native/.env.local");
    fs::write(&secret, "secret").unwrap();
    assert!(assembler::assemble(&root, &roots).is_err());
    fs::remove_file(secret).unwrap();
    let credentials = root.join("hosts/crates/host-native/credentials.json");
    fs::write(&credentials, "secret").unwrap();
    assert!(assembler::assemble(&root, &roots).is_err());
    fs::remove_file(credentials).unwrap();
    let binary = root.join("hosts/crates/host-native/opaque-data");
    fs::write(&binary, [0_u8, 1, 2]).unwrap();
    assert!(assembler::assemble(&root, &roots).is_err());
    fs::remove_file(binary).unwrap();

    let executable = root.join("hosts/crates/host-native/build-tool");
    fs::write(&executable, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(assembler::assemble(&root, &roots).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn manifest_must_equal_the_canonical_allowlist() {
    let root = fixture();
    let manifest = root.join("manifest.txt");
    fs::write(&manifest, "hosts/crates/unlisted\n").unwrap();
    assert!(assembler::read_roots(&manifest).is_err());
    fs::remove_dir_all(root).unwrap();
}
