use crate::input_shape::{
    reader_locale_supports_input, reader_locale_without_package_error,
    verify_input_is_package_shaped,
};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempPlainFile {
    path: PathBuf,
}

impl TempPlainFile {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!("faber-input-shape-{nonce}"));
        let _ = std::fs::write(&path, "temp");
        Self { path }
    }

    fn input(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for TempPlainFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn verify_input_is_package_shaped_accepts_faber_manifest_and_dirs() {
    assert!(verify_input_is_package_shaped(
        &[env!("CARGO_MANIFEST_DIR").to_owned()],
        false
    ));
    assert!(verify_input_is_package_shaped(
        &["faber.toml".to_owned()],
        false
    ));
    assert!(verify_input_is_package_shaped(
        &["pkg/faber.toml".to_owned()],
        false
    ));
}

#[test]
fn verify_input_is_package_shaped_rejects_stdin_and_single_source_files() {
    assert!(!verify_input_is_package_shaped(&["-".to_owned()], false));
    assert!(!verify_input_is_package_shaped(&["-".to_owned()], true));
    assert!(!verify_input_is_package_shaped(
        &["main.fab".to_owned()],
        false
    ));
    assert!(!verify_input_is_package_shaped(
        &["main.txt".to_owned()],
        false
    ));
    let file = TempPlainFile::new();
    assert!(!verify_input_is_package_shaped(&[file.input()], false));
    assert!(!verify_input_is_package_shaped(&[file.input()], true));
}

#[test]
fn verify_input_is_package_shaped_accepts_missing_extensionless_package_paths() {
    assert!(verify_input_is_package_shaped(
        &["missing-package".to_owned()],
        false
    ));
}

#[test]
fn reader_locale_supports_fab_entry_files_and_package_paths() {
    assert!(reader_locale_supports_input(
        &[env!("CARGO_MANIFEST_DIR").to_owned()],
        false
    ));
    assert!(reader_locale_supports_input(
        &["faber.toml".to_owned()],
        false
    ));
    assert!(reader_locale_supports_input(
        &["pkg/faber.toml".to_owned()],
        false
    ));
    assert!(reader_locale_supports_input(
        &["missing-package".to_owned()],
        false
    ));
    assert!(reader_locale_supports_input(
        &["main.fab".to_owned()],
        false
    ));
    assert!(!reader_locale_supports_input(&["-".to_owned()], false));
    assert!(!reader_locale_supports_input(
        &["main.txt".to_owned()],
        false
    ));
    let file = TempPlainFile::new();
    assert!(!reader_locale_supports_input(&[file.input()], false));
    assert!(!reader_locale_supports_input(
        &["main.fab".to_owned(), "other.fab".to_owned()],
        false
    ));
}

#[test]
fn reader_locale_without_package_error_only_rejects_unsupported_inputs() {
    assert_eq!(
        reader_locale_without_package_error(Some("la"), &["main.fab".to_owned()], false),
        None
    );
    assert_eq!(
        reader_locale_without_package_error(Some("la"), &["faber.toml".to_owned()], false),
        None
    );
    assert_eq!(
        reader_locale_without_package_error(Some("la"), &["pkg/faber.toml".to_owned()], false),
        None
    );
    assert_eq!(
        reader_locale_without_package_error(Some("la"), &["missing-package".to_owned()], false),
        None
    );
    assert_eq!(
        reader_locale_without_package_error(Some("la"), &["main.txt".to_owned()], false),
        Some("--reader-locale la requires a package path or .fab entry file".to_owned())
    );
    let file = TempPlainFile::new();
    assert_eq!(
        reader_locale_without_package_error(Some("la"), &[file.input()], false),
        Some("--reader-locale la requires a package path or .fab entry file".to_owned())
    );
    assert_eq!(
        reader_locale_without_package_error(Some("la"), &["-".to_owned()], true),
        Some("--reader-locale la requires a package path or .fab entry file".to_owned())
    );
    assert_eq!(
        reader_locale_without_package_error(
            Some("la"),
            &["main.fab".to_owned(), "other.fab".to_owned()],
            false
        ),
        Some("--reader-locale la requires a package path or .fab entry file".to_owned())
    );
    assert_eq!(
        reader_locale_without_package_error(None, &["main.fab".to_owned()], false),
        None
    );
}
