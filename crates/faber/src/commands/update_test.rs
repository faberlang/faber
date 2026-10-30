//! Tests for the Rust updater: release selection, checksum verification, the
//! versioned install layout, and uninstall. Network paths are exercised through
//! the pure release parser and a local fixture mirror.

use super::update::*;
use handroll::sha256_hex;
use semver::Version;
use std::fs;
use std::path::{Path, PathBuf};

fn release_json(tag: &str, prerelease: bool, draft: bool) -> ReleaseJson {
    serde_json::from_value(serde_json::json!({
        "tag_name": tag,
        "prerelease": prerelease,
        "draft": draft,
        "html_url": format!("https://example.invalid/{tag}"),
    }))
    .expect("release fixture")
}

#[test]
fn release_selection_ignores_other_components_and_prereleases() {
    let releases = vec![
        release_json("radix-v0.81.0", false, false),
        release_json("faber-v1.9.0", false, false),
        release_json("faber-v1.14.0", false, false),
        release_json("faber-v1.15.0-canary.1", true, false),
        release_json("faber-v1.16.0-rc.1", false, false),
        release_json("faber-v1.20.0", false, true),
        release_json("faber-v2.0.0", true, false),
        release_json("cista-v0.9.0", false, false),
    ];

    let mut best = None;
    for release in &releases {
        consider_release(&mut best, release);
    }
    let best = best.expect("one stable faber release");
    assert_eq!(best.version, Version::parse("1.14.0").expect("semver"));
    assert_eq!(best.tag, "faber-v1.14.0");
    assert_eq!(best.url, "https://example.invalid/faber-v1.14.0");
}

#[test]
fn semver_order_is_numeric() {
    let older = Version::parse("1.9.0").expect("semver");
    let newer = Version::parse("1.14.0").expect("semver");
    assert!(older < newer, "1.9.0 must sort before 1.14.0");
}

fn write_release(mirror: &Path, version: &str, entries: &[(&str, &[u8])], corrupt_checksum: bool) {
    let dir = mirror.join(format!("faber-v{version}"));
    fs::create_dir_all(&dir).expect("mirror dir");
    let archive_name = format!("faber-v{version}-aarch64-apple-darwin.tar.gz");
    let archive_path = dir.join(&archive_name);

    let file = fs::File::create(&archive_path).expect("archive file");
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *data)
            .expect("archive entry");
    }
    let encoder = builder.into_inner().expect("finish tar");
    encoder.finish().expect("finish gzip");

    let digest = sha256_hex(&fs::read(&archive_path).expect("read archive"));
    let digest = if corrupt_checksum {
        "0".repeat(64)
    } else {
        digest
    };
    let checksum_path = dir.join(format!("{archive_name}.sha256"));
    fs::write(checksum_path, format!("{digest}  {archive_name}\n")).expect("checksum");
}

#[test]
fn install_layout_places_a_versioned_binary_and_symlink() {
    let temp = tempfile::tempdir().expect("tmp");
    let mirror = temp.path().join("mirror");
    let bin_dir = temp.path().join("prefix").join("bin");
    write_release(&mirror, "1.0.0", &[("bin/faber", b"fake faber")], false);

    install_release(
        mirror.to_str().expect("mirror path"),
        &Version::parse("1.0.0").expect("semver"),
        "aarch64-apple-darwin",
        &bin_dir,
    )
    .expect("install");
    let versioned = bin_dir.join("faber-1.0.0");
    assert_eq!(
        fs::read(&versioned).expect("versioned binary"),
        b"fake faber"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&versioned)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755);
    }

    point_symlink(&bin_dir, &Version::parse("1.0.0").expect("semver")).expect("symlink");
    assert_eq!(
        fs::read_link(bin_dir.join("faber")).expect("link"),
        PathBuf::from("faber-1.0.0")
    );
    assert_eq!(active_version(&bin_dir).as_deref(), Some("faber-1.0.0"));
    assert_eq!(
        installed_versions(&bin_dir),
        vec![Version::parse("1.0.0").expect("semver")]
    );
}

#[test]
fn checksum_mismatch_leaves_no_binary() {
    let temp = tempfile::tempdir().expect("tmp");
    let mirror = temp.path().join("mirror");
    let bin_dir = temp.path().join("prefix").join("bin");
    write_release(&mirror, "1.0.0", &[("bin/faber", b"tampered")], true);

    let error = install_release(
        mirror.to_str().expect("mirror path"),
        &Version::parse("1.0.0").expect("semver"),
        "aarch64-apple-darwin",
        &bin_dir,
    )
    .expect_err("checksum mismatch must fail");
    assert!(error.contains("checksum mismatch"), "{error}");
    assert!(!bin_dir.join("faber-1.0.0").exists());
    assert!(!bin_dir.join(".faber-1.0.0.tmp").exists());
}

#[test]
fn archive_without_bin_faber_is_rejected() {
    let temp = tempfile::tempdir().expect("tmp");
    let mirror = temp.path().join("mirror");
    let bin_dir = temp.path().join("prefix").join("bin");
    write_release(&mirror, "1.0.0", &[("README", b"not a binary")], false);

    let error = install_release(
        mirror.to_str().expect("mirror path"),
        &Version::parse("1.0.0").expect("semver"),
        "aarch64-apple-darwin",
        &bin_dir,
    )
    .expect_err("missing bin/faber must fail");
    assert!(error.contains("does not contain bin/faber"), "{error}");
    assert!(!bin_dir.join("faber-1.0.0").exists());
}

#[test]
fn resolve_bin_dir_requires_a_managed_install() {
    let temp = tempfile::tempdir().expect("tmp");
    let exe = temp.path().join("faber");
    fs::write(&exe, b"cargo-installed").expect("exe");
    let error = resolve_bin_dir(None, Some(&exe)).expect_err("unmanaged binary must be refused");
    assert!(error.contains("no managed faber install"), "{error}");
    assert!(error.contains("--prefix"), "{error}");
}

#[cfg(unix)]
#[test]
fn resolve_bin_dir_accepts_a_versioned_install() {
    let temp = tempfile::tempdir().expect("tmp");
    let bin_dir = temp.path();
    fs::write(bin_dir.join("faber-1.0.0"), b"binary").expect("versioned");
    std::os::unix::fs::symlink("faber-1.0.0", bin_dir.join("faber")).expect("link");

    let resolved = resolve_bin_dir(None, Some(&bin_dir.join("faber-1.0.0"))).expect("managed");
    assert_eq!(resolved, bin_dir);
    let resolved = resolve_bin_dir(None, Some(&bin_dir.join("faber"))).expect("managed link");
    assert_eq!(resolved, bin_dir);
}

#[test]
fn resolve_bin_dir_accepts_an_explicit_prefix() {
    let temp = tempfile::tempdir().expect("tmp");
    let resolved = resolve_bin_dir(Some(temp.path()), None).expect("explicit prefix");
    assert_eq!(resolved, temp.path().join("bin"));
}

#[cfg(unix)]
#[test]
fn uninstall_one_version_keeps_the_other() {
    let temp = tempfile::tempdir().expect("tmp");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    fs::write(bin_dir.join("faber-1.0.0"), b"old").expect("1.0.0");
    fs::write(bin_dir.join("faber-1.1.0"), b"new").expect("1.1.0");
    std::os::unix::fs::symlink("faber-1.1.0", bin_dir.join("faber")).expect("link");

    run_uninstall(&UninstallArgs {
        version: Some("1.1.0".to_owned()),
        all: false,
        prefix: Some(temp.path().to_path_buf()),
    })
    .expect("uninstall one");

    assert!(bin_dir.join("faber-1.0.0").is_file());
    assert!(!bin_dir.join("faber-1.1.0").exists());
    assert!(fs::symlink_metadata(bin_dir.join("faber")).is_err());
}

#[cfg(unix)]
#[test]
fn uninstall_all_removes_a_dangling_link_without_installed_versions() {
    let temp = tempfile::tempdir().expect("tmp");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    std::os::unix::fs::symlink("faber-1.0.0", bin_dir.join("faber")).expect("dangling link");

    run_uninstall(&UninstallArgs {
        version: None,
        all: true,
        prefix: Some(temp.path().to_path_buf()),
    })
    .expect("uninstall all");

    assert!(fs::symlink_metadata(bin_dir.join("faber")).is_err());
}

#[cfg(unix)]
#[test]
fn uninstall_all_removes_every_version_and_link() {
    let temp = tempfile::tempdir().expect("tmp");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    fs::write(bin_dir.join("faber-1.0.0"), b"old").expect("1.0.0");
    fs::write(bin_dir.join("faber-1.1.0"), b"new").expect("1.1.0");
    std::os::unix::fs::symlink("faber-1.1.0", bin_dir.join("faber")).expect("link");

    run_uninstall(&UninstallArgs {
        version: None,
        all: true,
        prefix: Some(temp.path().to_path_buf()),
    })
    .expect("uninstall all");

    assert_eq!(installed_versions(&bin_dir), [] as [Version; 0]);
    assert!(fs::symlink_metadata(bin_dir.join("faber")).is_err());
}
