//! Validate `faber:*` kernel import paths during script-mode semantic analysis.
//!
//! Boundary: pre-collect AST pass. Rejects unknown or malformed `faber:*`
//! import paths (glob re-exports, non-flat paths, unknown modules). Collect
//! and later passes never see invalid kernel imports.

use crate::kernel::{
    KernelResolveError, is_kernel_glob_import_path, is_kernel_import_path, resolve_kernel_module,
};
use crate::{SemanticError, SemanticErrorKind};
use radix_lexer::Interner;
use radix_syntax::{ImportKind, Program, StmtKind, Visibility};

/// Reject unknown or malformed `faber:*` import paths before HIR lowering.
///
/// # Errors
///
/// Returns [`SemanticError`]s for non-kernel import paths in script mode.
pub fn validate_kernel_imports(
    program: &Program,
    interner: &Interner,
) -> Result<(), Vec<SemanticError>> {
    let mut errors = Vec::new();

    for stmt in &program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let path = interner.resolve(decl.path);
        if !is_kernel_import_path(path) {
            continue;
        }
        if is_kernel_glob_import_path(path) {
            if matches!(decl.visibility, Visibility::Publica) {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::ImportAbsent, decl.span)
                        .with_arg("issue", "kernel_glob_reexport")
                        .with_arg("path", "faber:*"),
                );
            }
            if !matches!(decl.kind, ImportKind::Named { .. }) {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::ImportAbsent, decl.span)
                        .with_arg("issue", "kernel_glob_named_alias")
                        .with_arg("path", "faber:*"),
                );
            }
            continue;
        }

        if let Err(err) = resolve_kernel_module(path) {
            let diagnostic = match err {
                KernelResolveError::NotKernelPath => continue,
                KernelResolveError::InvalidPath { reason, .. } => {
                    SemanticError::structured(SemanticErrorKind::ImportAbsent, decl.span)
                        .with_arg("issue", "kernel_invalid_path")
                        .with_arg("path", path)
                        .with_arg("reason", reason)
                }
                KernelResolveError::UnknownModule { name } => {
                    SemanticError::structured(SemanticErrorKind::ImportAbsent, decl.span)
                        .with_arg("issue", "kernel_unknown_module")
                        .with_arg("module", name)
                }
            };
            errors.push(diagnostic);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
