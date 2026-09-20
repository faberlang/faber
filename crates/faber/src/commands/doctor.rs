//! Read-only `faber doctor` — resolved library home and availability.
//!
//! Prints structured `key=value` lines. Does not mutate the environment, does
//! not attempt fixes, and does not parse `cista.toml`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use faber_cli::library::{
    KNOWN_LIBRARIES, LibraryAvailability, LibraryHomeSource, LibraryRootRule, library_availability,
    library_root_rule, resolved_library_home, specifier_identity,
};

/// One library row doctor emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorLibraryRow {
    pub name: String,
    pub identity: &'static str,
    pub availability: &'static str,
    pub root: Option<PathBuf>,
}

/// Structured doctor snapshot. Tests assert keys/values, not prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorReport {
    pub cwd: PathBuf,
    pub library_home: Option<PathBuf>,
    pub library_home_source: Option<LibraryHomeSource>,
    pub libraries: Vec<DoctorLibraryRow>,
}

/// Build the current doctor snapshot from ambient home resolution.
pub(crate) fn doctor_report() -> DoctorReport {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match resolved_library_home() {
        Some(home) => doctor_report_for(cwd, Some(home.path), Some(home.source)),
        None => doctor_report_for(cwd, None, None),
    }
}

pub(crate) fn doctor_report_for(
    cwd: PathBuf,
    library_home: Option<PathBuf>,
    library_home_source: Option<LibraryHomeSource>,
) -> DoctorReport {
    let mut names = BTreeSet::new();
    for name in KNOWN_LIBRARIES {
        names.insert((*name).to_owned());
    }
    if let Some(home) = library_home.as_ref() {
        if let Ok(entries) = std::fs::read_dir(home) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if radix::program::library::is_valid_provider_segment(name)
                    && library_availability(&path) != LibraryAvailability::NotFound
                {
                    names.insert(name.to_owned());
                }
            }
        }
    }

    let libraries = names
        .into_iter()
        .map(|name| library_row(library_home.as_deref(), &name))
        .collect();

    DoctorReport {
        cwd,
        library_home,
        library_home_source,
        libraries,
    }
}

fn library_row(library_home: Option<&Path>, name: &str) -> DoctorLibraryRow {
    let identity = specifier_identity(name).as_key();
    let Some(home) = library_home else {
        return DoctorLibraryRow {
            name: name.to_owned(),
            identity,
            availability: LibraryAvailability::NotFound.as_key(),
            root: None,
        };
    };
    let provider_repo = home.join(name);
    let availability = library_availability(&provider_repo);
    let root = match library_root_rule(&provider_repo) {
        Some(LibraryRootRule::FaberToml) => interface_root_for_faber_checkout(&provider_repo),
        Some(LibraryRootRule::SrcFallback) => Some(provider_repo.join("src")),
        None => None,
    };
    DoctorLibraryRow {
        name: name.to_owned(),
        identity,
        availability: availability.as_key(),
        root,
    }
}

fn interface_root_for_faber_checkout(provider_repo: &Path) -> Option<PathBuf> {
    let spec = faber_cli::package::discover_package(provider_repo).ok()?;
    Some(spec.source_root)
}

/// Render `key=value` lines. Empty optional values are omitted for `root`.
pub(crate) fn render_doctor_report(report: &DoctorReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("cwd={}\n", report.cwd.display()));
    match report.library_home.as_ref() {
        Some(path) => out.push_str(&format!("library_home={}\n", path.display())),
        None => out.push_str("library_home=\n"),
    }
    let source = report
        .library_home_source
        .map(LibraryHomeSource::as_key)
        .unwrap_or("unset");
    out.push_str(&format!("library_home_source={source}\n"));
    for row in &report.libraries {
        out.push_str(&format!(
            "library.{}.availability={}\n",
            row.name, row.availability
        ));
        out.push_str(&format!("library.{}.identity={}\n", row.name, row.identity));
        if let Some(root) = row.root.as_ref() {
            out.push_str(&format!("library.{}.root={}\n", row.name, root.display()));
        }
    }
    out
}

pub(super) fn cmd_doctor() {
    let report = doctor_report();
    print!("{}", render_doctor_report(&report));
}
