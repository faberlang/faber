use super::*;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct ConfiguredHomeReset;

impl ConfiguredHomeReset {
    fn clear() -> Self {
        radix::program::set_configured_library_home(None);
        Self
    }
}

impl Drop for ConfiguredHomeReset {
    fn drop(&mut self) {
        radix::program::set_configured_library_home(None);
    }
}

#[test]
fn embedded_library_home_holds_norma_when_home_unset() {
    let _guard = env_guard();
    let home = default_library_home();
    assert!(home.is_some());
    assert!(home.unwrap().join("norma/src/fs.fab").is_file());
}

#[test]
fn unset_home_records_embedded_provenance() {
    let _guard = env_guard();
    let resolved = resolved_library_home().expect("embedded home");
    assert_eq!(resolved.source, LibraryHomeSource::Embedded);
    assert!(resolved.path.join("norma/src/fs.fab").is_file());
}

#[test]
fn emit_gate_installs_an_embedded_home_that_serves_norma_text() {
    // The relocated-binary refusal: with no in-tree `lib/` and no configured
    // home, the single-file emit lanes refused `norma:*` imports with PKG001
    // `unknown_library_module`. The dispatch gate must install the embedded
    // home as the configured setting, and that home must hold the refused
    // module surface.
    let _guard = env_guard();
    let embedded = resolved_library_home().expect("embedded home");
    assert_eq!(embedded.source, LibraryHomeSource::Embedded);

    let _configured = ConfiguredHomeReset::clear();
    configure_embedded_library_home_if_unset();

    let installed = radix::program::configured_library_home_setting()
        .expect("the emit gate installs the embedded home");
    assert_eq!(installed, embedded.path);
    assert!(
        installed.join("norma/src/text.fab").is_file(),
        "the installed home must hold the norma:text module"
    );
}

#[test]
fn configure_library_home_flag_wins_over_manifest() {
    let _guard = env_guard();
    let _configured = ConfiguredHomeReset::clear();

    let root = tempfile::tempdir().expect("temp");
    let package = root.path().join("pkg");
    std::fs::create_dir_all(&package).expect("package dir");
    std::fs::write(
        package.join("faber.toml"),
        "[paths]\nlibrary_home = \"manifest-home\"\n",
    )
    .expect("manifest");
    let flag = root.path().join("flag-home");

    configure_library_home(Some(flag.as_path()), Some(package.as_path()));

    let resolved = resolved_library_home().expect("configured home");
    assert_eq!(resolved.path, flag);
    assert_eq!(resolved.source, LibraryHomeSource::Configured);
    assert_eq!(LibraryHomeSource::Configured.as_key(), "configured");
}

#[test]
fn configure_library_home_makes_a_relative_flag_absolute() {
    let _guard = env_guard();
    let _configured = ConfiguredHomeReset::clear();

    configure_library_home(Some(Path::new("../relative-home")), None);

    let resolved = resolved_library_home().expect("configured home");
    assert!(resolved.path.is_absolute(), "{:?}", resolved.path);
    assert_eq!(
        resolved.path,
        std::env::current_dir()
            .expect("current directory")
            .join("../relative-home")
    );
}

#[test]
fn configure_library_home_reads_manifest_paths_key() {
    let _guard = env_guard();
    let _configured = ConfiguredHomeReset::clear();

    let root = tempfile::tempdir().expect("temp");
    let package = root.path().join("pkg");
    std::fs::create_dir_all(package.join("src")).expect("package src");
    std::fs::write(
        package.join("faber.toml"),
        "[paths]\nlibrary_home = \"vendor/libs\"\n",
    )
    .expect("manifest");
    let probe = package.join("src/main.fab");

    configure_library_home(None, Some(probe.as_path()));

    let resolved = resolved_library_home().expect("manifest home");
    assert_eq!(resolved.path, package.join("vendor/libs"));
    assert_eq!(resolved.source, LibraryHomeSource::Configured);
}

#[test]
fn specifier_identity_is_not_manifest_derived() {
    assert_eq!(specifier_identity("norma"), SpecifierIdentity::Builtin);
    assert_eq!(specifier_identity("triga"), SpecifierIdentity::Package);
    assert_eq!(specifier_identity("gradus"), SpecifierIdentity::Package);
    assert_eq!(specifier_identity("tela"), SpecifierIdentity::Package);
    assert_eq!(
        specifier_identity("anything-else"),
        SpecifierIdentity::Package
    );
}

#[test]
fn availability_uses_file_presence_and_does_not_need_cista_parse() {
    let root = tempfile::tempdir().expect("temp");
    let faber_pkg = root.path().join("triga");
    std::fs::create_dir_all(faber_pkg.join("src")).expect("triga src");
    std::fs::write(
        faber_pkg.join("faber.toml"),
        "[package]\nname = \"triga\"\n",
    )
    .expect("triga manifest");
    assert_eq!(
        library_availability(&faber_pkg),
        LibraryAvailability::FaberPackage
    );

    let cista = root.path().join("norma");
    std::fs::create_dir_all(cista.join("src")).expect("norma src");
    std::fs::write(cista.join("cista.toml"), "this is not toml {{{").expect("garbage cista");
    assert_eq!(
        library_availability(&cista),
        LibraryAvailability::CistaSourcePackage
    );

    assert_eq!(
        library_availability(&root.path().join("missing")),
        LibraryAvailability::NotFound
    );
    assert_eq!(LibraryAvailability::FaberPackage.as_key(), "faber-package");
    assert_eq!(
        LibraryAvailability::CistaSourcePackage.as_key(),
        "cista-source-package"
    );
    assert_eq!(SpecifierIdentity::Builtin.as_key(), "builtin");
    assert!(KNOWN_LIBRARIES.contains(&"norma"));
    assert_eq!(
        library_root_rule(&faber_pkg),
        Some(LibraryRootRule::FaberToml)
    );
    assert_eq!(
        library_root_rule(&cista),
        Some(LibraryRootRule::SrcFallback)
    );
    assert_eq!(LibraryRootRule::SrcFallback.as_key(), "src-fallback");
}
