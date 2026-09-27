//! Driver-side resolution of build-time file embeds (DECISIONS D8.10).
//!
//! `generis T X = insere "path"` reads a neighbouring file at build time.
//! Semantic analysis stays free of file-system access, so the driver resolves
//! every authored embed path first and hands the bytes (or the rejection) to
//! lowering through [`InsereFiles`].
//!
//! PATH RULES
//! ==========
//! A path is **package-relative**: it resolves against the package root, the
//! nearest ancestor directory of the source file holding `faber.toml` — the
//! same discovery radix uses for package layout. A single-file compile with
//! no manifest resolves against the file's own directory. Absolute paths are
//! rejected, and no path may leave the root, lexically through `..` or
//! physically through a symlink. Each resolved file is recorded as a build
//! input on the analyzed module so a package driver can rebuild when it
//! changes.

use std::path::{Component, Path, PathBuf};

use crate::semantic::insere::{InsereFiles, InsereRejection, InsereSource};
use radix_lexer::Interner;
use radix_syntax::{ClassMemberKind, ExprKind, Program, StmtKind};

const MANIFEST_FILE: &str = "faber.toml";

/// Embed table plus the canonical files it read (the unit's build inputs).
pub(crate) struct ResolvedInsere {
    pub files: InsereFiles,
    pub inputs: Vec<PathBuf>,
}

/// Resolve every `insere` path authored in `program`'s `generis` initializers.
pub(crate) fn resolve_insere_files(program: &Program, interner: &Interner, source_name: &str) -> ResolvedInsere {
    let mut resolved = ResolvedInsere { files: InsereFiles::new(), inputs: Vec::new() };
    let paths = authored_insere_paths(program, interner);
    if paths.is_empty() {
        return resolved;
    }
    let root = embed_root(Path::new(source_name));
    for path in paths {
        let source = match &root {
            Some(root) => read_embed(root, &path, &mut resolved.inputs),
            None => InsereSource::Rejected(InsereRejection::NoSourceRoot),
        };
        resolved.files.insert(path, source);
    }
    resolved
}

/// Authored embed paths in top-level statics and genus static fields.
fn authored_insere_paths(program: &Program, interner: &Interner) -> Vec<String> {
    let mut paths = Vec::new();
    let mut push = |kind: &ExprKind| {
        if let ExprKind::Insere(insere) = kind {
            paths.push(interner.resolve(insere.path).to_owned());
        }
    };
    for stmt in &program.statements {
        match &stmt.kind {
            StmtKind::Var(decl) if decl.is_static => {
                if let Some(init) = &decl.init {
                    push(&init.kind);
                }
            }
            StmtKind::Class(class) => {
                for member in &class.members {
                    if let ClassMemberKind::Field(field) = &member.kind
                        && let Some(init) = &field.init
                    {
                        push(&init.kind);
                    }
                }
            }
            _ => {}
        }
    }
    paths
}

/// The canonical package root for a source file, or `None` when the source
/// is not a file on disk (stdin, in-memory analysis).
fn embed_root(source: &Path) -> Option<PathBuf> {
    if !source.is_file() {
        return None;
    }
    let dir = source.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let dir = dir.canonicalize().ok()?;
    let root = dir
        .ancestors()
        .find(|candidate| candidate.join(MANIFEST_FILE).is_file())
        .map_or_else(|| dir.clone(), Path::to_path_buf);
    Some(root)
}

/// Read one embed path against the canonical `root`.
fn read_embed(root: &Path, authored: &str, inputs: &mut Vec<PathBuf>) -> InsereSource {
    let relative = match package_relative(authored) {
        Ok(relative) => relative,
        Err(rejection) => return InsereSource::Rejected(rejection),
    };
    let canonical = match root.join(relative).canonicalize() {
        Ok(canonical) => canonical,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return InsereSource::Rejected(InsereRejection::Missing);
        }
        Err(_) => return InsereSource::Rejected(InsereRejection::Unreadable),
    };
    if !canonical.starts_with(root) {
        return InsereSource::Rejected(InsereRejection::EscapesPackage);
    }
    match std::fs::read(&canonical) {
        Ok(bytes) => {
            inputs.push(canonical);
            InsereSource::Bytes(bytes)
        }
        Err(_) => InsereSource::Rejected(InsereRejection::Unreadable),
    }
}

/// Lexically normalize an authored path, rejecting absolute paths and any
/// `..` that climbs above the package root.
fn package_relative(authored: &str) -> Result<PathBuf, InsereRejection> {
    let path = Path::new(authored);
    if authored.starts_with(['/', '\\']) || path.has_root() || path.is_absolute() {
        return Err(InsereRejection::AbsolutePath);
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(InsereRejection::EscapesPackage);
                }
            }
            Component::RootDir | Component::Prefix(_) => return Err(InsereRejection::AbsolutePath),
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(InsereRejection::Missing);
    }
    Ok(normalized)
}

#[cfg(test)]
#[path = "insere_test.rs"]
mod tests;
