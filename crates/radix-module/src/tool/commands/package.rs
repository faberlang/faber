//! Package-mode heuristics for CLI input path detection.
//!
//! Owns [`should_treat_as_package_from_input`] which wraps
//! [`super::compile::should_treat_as_package`] for the CLI argument shape.
//! Delegates no compilation; other commands consult this to decide whether an
//! input names a directory / manifest and should be rejected with a `faber`
//! redirect.
//!
//! Nucleum fragment imports also fail closed here: a signature-only fragment
//! interface is a compile error, and only `Entry` names enter dispatch /
//! reflection tables.

use super::compile::should_treat_as_package;

pub(crate) fn should_treat_as_package_from_input(input: &[String]) -> bool {
    if input.is_empty() || input[0] == "-" {
        return false;
    }
    let path = std::path::Path::new(&input[0]);
    should_treat_as_package(path)
}

/// Names that may enter device dispatch and reflection tables (`Entry` only).
#[cfg(feature = "hir-fhir")]
#[must_use]
pub fn nucleum_dispatch_reflection_names(metadata: &crate::hir::package::NucleumProgramMetadata) -> Vec<String> {
    metadata.dispatch_and_reflection_names()
}

/// Fragment bodies omitted from target symbol tables after composition.
#[cfg(feature = "hir-fhir")]
#[must_use]
pub fn nucleum_omitted_target_symbols(metadata: &crate::hir::package::NucleumProgramMetadata) -> Vec<String> {
    metadata.omitted_target_symbol_names()
}

/// Compile-time diagnostic for a signature-only fragment import.
#[cfg(feature = "hir-fhir")]
#[must_use]
pub fn signature_only_fragment_import_diagnostic(file: &str, name: &str) -> crate::diagnostics::Diagnostic {
    crate::diagnostics::Diagnostic::error(format!("signature-only nucleum fragment `{name}` cannot be imported"))
        .with_code("PKG001")
        .with_file(file.to_owned())
        .with_arg("issue", "NUCLEUM_FRAGMENT_EFFECT")
        .with_arg("reason", "missing_body")
        .with_arg("site", "import")
}

#[cfg(all(test, feature = "hir-fhir"))]
mod tests {
    use super::*;
    use crate::hir::package::{NucleumItemMetadata, NucleumProgramMetadata};
    use crate::hir::NucleumRole;

    #[test]
    fn dispatch_tables_exclude_fragments() {
        let metadata = NucleumProgramMetadata {
            compiler_version: crate::hir::package::NUCLEUM_COMPILER_VERSION.to_owned(),
            abi_version: crate::hir::package::NUCLEUM_FRAGMENT_ABI_VERSION,
            items: vec![
                NucleumItemMetadata {
                    name: "k".to_owned(),
                    relative_path: "main.fab".to_owned(),
                    def_id: 1,
                    role: NucleumRole::Entry,
                    has_hir_body: true,
                    body: None,
                    fragment_callees: Vec::new(),
                },
                NucleumItemMetadata {
                    name: "helper".to_owned(),
                    relative_path: "main.fab".to_owned(),
                    def_id: 2,
                    role: NucleumRole::Fragment,
                    has_hir_body: true,
                    body: None,
                    fragment_callees: Vec::new(),
                },
            ],
        };
        assert_eq!(nucleum_dispatch_reflection_names(&metadata), vec!["k".to_owned()]);
        assert_eq!(nucleum_omitted_target_symbols(&metadata), vec!["helper".to_owned()]);
    }

    #[test]
    fn signature_only_import_diagnostic_is_fail_closed() {
        let diagnostic = signature_only_fragment_import_diagnostic("lib.fab", "helper");
        assert_eq!(
            diagnostic
                .args
                .iter()
                .find(|arg| arg.name == "issue")
                .map(|arg| arg.value.as_str()),
            Some("NUCLEUM_FRAGMENT_EFFECT")
        );
        assert!(diagnostic.message.contains("helper"));
    }
}
