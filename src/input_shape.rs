pub(crate) fn verify_input_is_package_shaped(input: &[String], _force_package: bool) -> bool {
    let Some(first) = input.first() else {
        return false;
    };
    if first == "-" {
        return false;
    }
    let path = std::path::Path::new(first);
    path.is_dir()
        || path.file_name().is_some_and(|name| name == "faber.toml")
        || (!path.exists() && path.extension().is_none())
}

pub(crate) fn reader_locale_supports_input(input: &[String], force_package: bool) -> bool {
    if input.len() != 1 {
        return false;
    }
    if verify_input_is_package_shaped(input, force_package) {
        return true;
    }
    let Some(first) = input.first() else {
        return false;
    };
    if first == "-" {
        return false;
    }
    std::path::Path::new(first)
        .extension()
        .is_some_and(|ext| ext == "fab")
}

pub(crate) fn reader_locale_without_package_error(
    reader_locale: Option<&str>,
    input: &[String],
    force_package: bool,
) -> Option<String> {
    let locale = reader_locale?;
    if reader_locale_supports_input(input, force_package) {
        return None;
    }
    Some(format!(
        "--reader-locale {locale} requires a package path or .fab entry file"
    ))
}
