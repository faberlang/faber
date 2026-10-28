//! The build target a target-native import binds the program to.
//!
//! `import python "math" m` names the target that will compile the program.
//! A build for any other target is SEM017 `import_target_bound`. The check is
//! target qualification, not analysis: the import's target word is HIR data, so
//! it runs wherever the build target is known, over a freshly analyzed unit or
//! one loaded from a FHIR artifact (which is written without a build target
//! and is never refused here).

use crate::{SemanticError, SemanticErrorKind};
use radix_hir::{HirImport, HirItemKind, HirModule};
use radix_lexer::{Interner, Span};
use radix_target::Target;

/// The `import_target_bound` refusal of one target-native `import` when
/// `build` is not the target it names; `None` when it matches (or the import
/// names no target).
#[must_use]
pub fn mismatch(
    import: &HirImport,
    span: Span,
    interner: &Interner,
    build: Target,
) -> Option<SemanticError> {
    let word = interner.resolve(import.target?);
    if Target::from_import_word(word).is_some_and(|bound| bound == build) {
        return None;
    }
    Some(
        SemanticError::structured(SemanticErrorKind::UnavailableDeclaration, span)
            .with_arg("issue", "import_target_bound")
            .with_arg("target", word.to_owned())
            .with_arg("specifier", interner.resolve(import.path).to_owned())
            .with_arg(
                "build",
                if build == Target::MirStepper {
                    "runner".to_owned()
                } else {
                    build.as_str().to_owned()
                },
            ),
    )
}

/// Every target-native import of `hir` that `build` refuses, in source order.
#[must_use]
pub fn check(hir: &HirModule, interner: &Interner, build: Target) -> Vec<SemanticError> {
    hir.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Import(import) => mismatch(import, item.span, interner, build),
            _ => None,
        })
        .collect()
}
