use radix::diagnostics::Diagnostic;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

type ArchiveResult<T> = Result<T, Box<Diagnostic>>;

pub(super) struct ExtractedArchivePackage {
    archive: PathBuf,
    root: PathBuf,
    package_input: PathBuf,
}

impl ExtractedArchivePackage {
    pub(super) fn package_input(&self) -> &Path {
        &self.package_input
    }

    pub(super) fn remap_diagnostics(&self, diagnostics: &mut [Diagnostic]) {
        for diagnostic in diagnostics {
            let file = Path::new(&diagnostic.file);
            let Ok(member) = file.strip_prefix(&self.root) else {
                continue;
            };
            diagnostic.file = format!("{}!/{}", self.archive.display(), member.display());
        }
    }
}

impl Drop for ExtractedArchivePackage {
    fn drop(&mut self) {
        remove_dir_all_best_effort(&self.root);
    }
}

pub(super) fn is_zip_archive(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("zip")
}

pub(super) fn extract_package_archive(archive: &Path) -> ArchiveResult<ExtractedArchivePackage> {
    let archive = archive.to_path_buf();
    let temp_root = create_temp_root(&archive)?;
    let extraction = extract_zip(&archive, &temp_root).and_then(|()| {
        select_package_root(&archive, &temp_root).map(|package_input| ExtractedArchivePackage {
            archive: archive.clone(),
            root: temp_root.clone(),
            package_input,
        })
    });

    if extraction.is_err() {
        remove_dir_all_best_effort(&temp_root);
    }
    extraction
}

fn remove_dir_all_best_effort(path: &Path) {
    if let Err(err) = fs::remove_dir_all(path) {
        if err.kind() != io::ErrorKind::NotFound {
            eprintln!(
                "warning: could not remove archive extraction directory `{}`: {err}",
                path.display()
            );
        }
    }
}

fn create_temp_root(archive: &Path) -> ArchiveResult<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let stem = archive
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("archive");
    let base = std::env::temp_dir().join(format!(
        "faber-archive-{}-{}-{nanos}",
        std::process::id(),
        sanitize_temp_label(stem)
    ));

    for attempt in 0..16 {
        let candidate = if attempt == 0 {
            base.clone()
        } else {
            base.with_extension(attempt.to_string())
        };
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(Box::new(Diagnostic::io_error(&candidate, err)));
            }
        }
    }

    Err(Box::new(
        crate::package_diagnostic_error("could not create a unique archive extraction directory")
            .with_file(archive.display().to_string()),
    ))
}

fn sanitize_temp_label(label: &str) -> String {
    let sanitized = label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "archive".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn extract_zip(archive: &Path, temp_root: &Path) -> ArchiveResult<()> {
    let file =
        fs::File::open(archive).map_err(|err| Box::new(Diagnostic::io_error(archive, err)))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|err| {
        Box::new(
            crate::package_diagnostic_error(format!("cannot read zip archive: {err}"))
                .with_file(archive.display().to_string()),
        )
    })?;

    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|err| {
            Box::new(
                crate::package_diagnostic_error(format!(
                    "cannot read zip archive entry {index}: {err}"
                ))
                .with_file(archive.display().to_string()),
            )
        })?;
        let entry_name = entry.name().to_owned();
        if is_zip_symlink(&entry) {
            return Err(unsafe_entry_diagnostic(archive, &entry_name));
        }
        let relative = safe_archive_path(&entry_name)
            .map_err(|()| unsafe_entry_diagnostic(archive, &entry_name))?;
        let output = temp_root.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)
                .map_err(|err| Box::new(Diagnostic::io_error(&output, err)))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| Box::new(Diagnostic::io_error(parent, err)))?;
        }
        let mut out = fs::File::create(&output)
            .map_err(|err| Box::new(Diagnostic::io_error(&output, err)))?;
        io::copy(&mut entry, &mut out)
            .map_err(|err| Box::new(Diagnostic::io_error(&output, err)))?;
    }

    Ok(())
}

fn is_zip_symlink(entry: &zip::read::ZipFile<'_>) -> bool {
    entry
        .unix_mode()
        .is_some_and(|mode| (mode & 0o170000) == 0o120000)
}

fn safe_archive_path(name: &str) -> Result<PathBuf, ()> {
    if name.trim().is_empty() || name.contains('\\') {
        return Err(());
    }
    let path = Path::new(name);
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir if out.as_os_str().is_empty() => {}
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(()),
        }
    }
    if out.as_os_str().is_empty() {
        Err(())
    } else {
        Ok(out)
    }
}

fn unsafe_entry_diagnostic(archive: &Path, entry: &str) -> Box<Diagnostic> {
    Box::new(
        crate::package_diagnostic_error(format!("unsafe archive entry `{entry}`"))
            .with_file(archive.display().to_string()),
    )
}

fn select_package_root(archive: &Path, temp_root: &Path) -> ArchiveResult<PathBuf> {
    if temp_root.join("faber.toml").is_file() || temp_root.join("main.fab").is_file() {
        return Ok(temp_root.to_path_buf());
    }

    let top_dirs = top_level_dirs(temp_root)?;
    if let [dir] = top_dirs.as_slice() {
        return Ok(temp_root.join(dir));
    }

    Err(Box::new(
        crate::package_diagnostic_error(
            "archive package root must contain `faber.toml`, `main.fab`, or one top-level package directory",
        )
        .with_file(archive.display().to_string()),
    ))
}

fn top_level_dirs(temp_root: &Path) -> ArchiveResult<Vec<PathBuf>> {
    let mut dirs = BTreeSet::new();
    for entry in
        fs::read_dir(temp_root).map_err(|err| Box::new(Diagnostic::io_error(temp_root, err)))?
    {
        let entry = entry.map_err(|err| Box::new(Diagnostic::io_error(temp_root, err)))?;
        if entry
            .file_type()
            .map_err(|err| Box::new(Diagnostic::io_error(&entry.path(), err)))?
            .is_dir()
        {
            dirs.insert(PathBuf::from(entry.file_name()));
        }
    }
    Ok(dirs.into_iter().collect())
}
