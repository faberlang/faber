//! Program-wide `@ ad` route table (A3/U4, D6.10, fork F11 default).
//!
//! Tier 1 of the conversation router is a static table the compiler builds
//! from every `@ ad '<route>'` handler in the program's static link set: the
//! entry package's analyzed modules plus every library module in their
//! (transitively expanded) import closure. Importing a module links its
//! handlers even when no symbol from it is referenced (Q6: module
//! granularity, not whole packages).
//!
//! Each module's routes come from its [`FileInterface::routes`] facts
//! (per-file legality and in-file duplicates are `passes::ad_routes` work).
//! This module merges them and reports a route served by two different
//! modules — in one package or across packages — as `SEM005`
//! `ad_route_duplicate`, naming both definitions.
//!
//! Backend emission (Rust, Go, TS, runner) consumes the table in later
//! units; here it is a compiler fact on [`crate::analyze::AnalyzedProgram`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use radix_module::diagnostics::{Diagnostic, DiagnosticPhase};
use radix_module::file_interface::RouteExport;
use radix_module::semantic::SemanticErrorKind;

/// Where one route handler is defined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteOrigin {
    /// Package that owns the defining module (`None` for an unnamed package).
    pub package: Option<String>,
    /// Defining source or library interface file.
    pub path: PathBuf,
}

impl RouteOrigin {
    fn display(&self, line: Option<usize>) -> String {
        let location = match line {
            Some(line) => format!("{}:{line}", self.path.display()),
            None => self.path.display().to_string(),
        };
        match &self.package {
            Some(package) => format!("`{package}` ({location})"),
            None => location,
        }
    }
}

/// One tier-1 route: its handler facts plus the defining module.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteEntry {
    pub export: RouteExport,
    pub origin: RouteOrigin,
}

/// The static Faber handler table: exact route → handler.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteTable {
    entries: BTreeMap<String, RouteEntry>,
}

impl RouteTable {
    /// The handler serving `route`, when tier 1 answers it.
    #[must_use]
    pub fn get(&self, route: &str) -> Option<&RouteEntry> {
        self.entries.get(route)
    }

    /// Whether tier 1 serves `route`.
    #[must_use]
    pub fn serves(&self, route: &str) -> bool {
        self.entries.contains_key(route)
    }

    /// Every served route in route order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &RouteEntry)> {
        self.entries.iter()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Accumulates module route sets and reports cross-module duplicates.
#[derive(Default)]
pub struct RouteTableBuilder {
    table: RouteTable,
    /// Modules already merged, so a library module reached from several
    /// importers contributes its routes once.
    merged: BTreeSet<PathBuf>,
    diagnostics: Vec<Diagnostic>,
}

impl RouteTableBuilder {
    /// Merge one module's routes. A module path seen before is skipped.
    pub fn merge_module(&mut self, origin: &RouteOrigin, routes: &BTreeMap<String, RouteExport>) {
        if !self.merged.insert(origin.path.clone()) {
            return;
        }
        for (route, export) in routes {
            if let Some(first) = self.table.entries.get(route) {
                self.diagnostics
                    .push(duplicate_route_diagnostic(first, origin, export));
                continue;
            }
            self.table.entries.insert(
                route.clone(),
                RouteEntry {
                    export: export.clone(),
                    origin: origin.clone(),
                },
            );
        }
    }

    /// The merged table, or every duplicate-route diagnostic.
    ///
    /// # Errors
    /// Returns the duplicate-route diagnostics when two modules serve one route.
    pub fn finish(self) -> Result<RouteTable, Vec<Diagnostic>> {
        if self.diagnostics.is_empty() {
            Ok(self.table)
        } else {
            Err(self.diagnostics)
        }
    }
}

/// Rebuild the table from analyzed package modules alone (FHIR rehydration,
/// where no library resolver exists). Library-served routes are absent from
/// this table; callers that have a resolver use package analysis instead.
///
/// # Errors
/// Returns duplicate-route or snapshot diagnostics.
pub fn local_route_table<'a, I>(
    modules: I,
    package: Option<&String>,
) -> Result<RouteTable, Vec<Diagnostic>>
where
    I: IntoIterator<Item = (&'a Path, &'a radix_module::driver::AnalyzedModule)>,
{
    let mut builder = RouteTableBuilder::default();
    for (path, analysis) in modules {
        let routes = radix_module::semantic::passes::ad_routes::route_exports(
            &analysis.hir,
            &analysis.types,
            &analysis.interner,
            &analysis.resolver,
            None,
        )
        .map_err(|err| {
            vec![
                crate::package_diagnostic_error(format!(
                    "`@ ad` route handler cannot be snapshotted: {err:?}"
                ))
                .with_file(path.display().to_string())
                .with_arg("issue", "ad_route_snapshot_failed"),
            ]
        })?;
        let origin = RouteOrigin {
            package: package.cloned(),
            path: path.to_path_buf(),
        };
        builder.merge_module(&origin, &routes);
    }
    builder.finish()
}

fn duplicate_route_diagnostic(
    first: &RouteEntry,
    second_origin: &RouteOrigin,
    second: &RouteExport,
) -> Diagnostic {
    let first_line = source_line(&first.origin.path, first.export.span.start);
    let second_source = read_source(&second_origin.path);
    let first_label = first.origin.display(first_line);
    let second_label = second_origin.display(
        second_source
            .as_ref()
            .map(|(raw, offset)| line_of(raw, offset + second.span.start as usize)),
    );
    let mut diagnostic = Diagnostic::error(format!(
        "route '{}' is served by both {first_label} and {second_label}",
        second.route
    ))
    .with_phase(DiagnosticPhase::Resolve)
    .with_code("SEM005")
    .with_semantic_kind(SemanticErrorKind::DefinitionDuplicate)
    .with_arg("issue", "ad_route_duplicate")
    .with_arg("route", second.route.clone())
    .with_arg("first", first_label)
    .with_arg("second", second_label);
    let file = second_origin.path.display().to_string();
    diagnostic = match &second_source {
        Some((raw, offset)) => {
            let span = radix_module::lexer::Span::new(
                second.span.start + *offset as u32,
                second.span.end + *offset as u32,
            );
            diagnostic.with_source_context(&file, raw, span)
        }
        None => diagnostic.with_file(file),
    };
    diagnostic
}

/// Raw file text plus the byte offset of the peeled body (spans are
/// body-relative).
fn read_source(path: &Path) -> Option<(String, usize)> {
    let raw = std::fs::read_to_string(path).ok()?;
    let offset = radix_module::driver::peel_raw_source(&path.display().to_string(), &raw)
        .ok()
        .map_or(0, |peeled| peeled.body_byte_offset as usize);
    Some((raw, offset))
}

fn source_line(path: &Path, body_offset: u32) -> Option<usize> {
    read_source(path).map(|(raw, offset)| line_of(&raw, offset + body_offset as usize))
}

fn line_of(raw: &str, byte: usize) -> usize {
    raw.as_bytes()[..byte.min(raw.len())]
        .split(|&b| b == b'\n')
        .count()
}

#[cfg(test)]
#[path = "route_table_test.rs"]
mod tests;
