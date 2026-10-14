use super::*;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct EnvRestore {
    home: Option<std::ffi::OsString>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            home: std::env::var_os(FABER_LIBRARY_HOME_ENV),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env(FABER_LIBRARY_HOME_ENV, self.home.take());
    }
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    // SAFETY: serialized by ENV_LOCK; edition 2024 marks env mutation unsafe.
    match value {
        Some(value) => unsafe {
            std::env::set_var(key, value);
        },
        None => unsafe {
            std::env::remove_var(key);
        },
    }
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
    let _env = EnvRestore::capture();
    // SAFETY: serialized by ENV_LOCK; edition 2024 marks env mutation unsafe.
    unsafe {
        std::env::remove_var(FABER_LIBRARY_HOME_ENV);
    }

    let home = default_library_home();
    assert!(home.is_some());
    assert!(home.unwrap().join("norma/src/fs.fab").is_file());
}

#[test]
fn explicit_library_home_wins_over_workspace_probe() {
    let _guard = env_guard();
    let _env = EnvRestore::capture();
    let explicit = PathBuf::from("/tmp/faber-explicit-library-home-test");
    // SAFETY: serialized by ENV_LOCK; edition 2024 marks env mutation unsafe.
    unsafe {
        std::env::set_var(FABER_LIBRARY_HOME_ENV, &explicit);
    }

    assert_eq!(default_library_home(), Some(explicit.clone()));
    let resolved = resolved_library_home().expect("env home");
    assert_eq!(resolved.path, explicit);
    assert_eq!(resolved.source, LibraryHomeSource::Env);
}

#[test]
fn unset_home_records_embedded_provenance() {
    let _guard = env_guard();
    let _env = EnvRestore::capture();
    // SAFETY: serialized by ENV_LOCK; edition 2024 marks env mutation unsafe.
    unsafe {
        std::env::remove_var(FABER_LIBRARY_HOME_ENV);
    }

    let resolved = resolved_library_home().expect("embedded home");
    assert_eq!(resolved.source, LibraryHomeSource::Embedded);
    assert!(resolved.path.join("norma/src/fs.fab").is_file());
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
    assert_eq!(LibraryHomeSource::Env.as_key(), "env");
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
