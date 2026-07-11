pub(crate) fn verify_input_is_package_shaped(input: &[String], force_package: bool) -> bool {
    if force_package {
        return true;
    }
    let Some(first) = input.first() else {
        return false;
    };
    if first == "-" {
        return false;
    }
    let path = std::path::Path::new(first);
    path.is_dir() || path.file_name().is_some_and(|name| name == "faber.toml")
}

pub(crate) fn reader_locale_without_package_error(
    reader_locale: Option<&str>,
    input: &[String],
    force_package: bool,
) -> Option<String> {
    let locale = reader_locale?;
    if verify_input_is_package_shaped(input, force_package) {
        return None;
    }
    Some(format!(
        "--reader-locale {locale} requires a package path or .fab entry file"
    ))
}

#[cfg(test)]
mod tests {
    use super::{reader_locale_without_package_error, verify_input_is_package_shaped};

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
    }

    #[test]
    fn verify_input_is_package_shaped_rejects_stdin_and_single_source_files() {
        assert!(!verify_input_is_package_shaped(&["-".to_owned()], false));
        assert!(!verify_input_is_package_shaped(
            &["main.fab".to_owned()],
            false
        ));
    }

    #[test]
    fn reader_locale_without_package_error_only_rejects_non_package_inputs() {
        assert_eq!(
            reader_locale_without_package_error(Some("la"), &["main.fab".to_owned()], false),
            Some("--reader-locale la requires a package path or .fab entry file".to_owned())
        );
        assert_eq!(
            reader_locale_without_package_error(Some("la"), &["faber.toml".to_owned()], false),
            None
        );
        assert_eq!(
            reader_locale_without_package_error(Some("la"), &["-".to_owned()], true),
            None
        );
        assert_eq!(
            reader_locale_without_package_error(None, &["main.fab".to_owned()], false),
            None
        );
    }
}
