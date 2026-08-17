//! Package-mode heuristics for CLI input path detection.
//!
//! Owns [`should_treat_as_package_from_input`] which wraps
//! [`super::compile::should_treat_as_package`] for the CLI argument shape.
//! Delegates no compilation; other commands consult this to decide whether an
//! input names a directory / manifest and should be rejected with a `faber`
//! redirect.

use super::compile::should_treat_as_package;

pub(crate) fn should_treat_as_package_from_input(input: &[String]) -> bool {
    if input.is_empty() || input[0] == "-" {
        return false;
    }
    let path = std::path::Path::new(&input[0]);
    should_treat_as_package(path)
}
