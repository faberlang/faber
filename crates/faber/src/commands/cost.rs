//! `faber cost` — static runtime-cost report over lowered MIR.
//!
//! Compiles a package to [`ValidatedMir`] through the same discovery/config
//! front half as `faber check`, then classifies conversion and uncosted-extern
//! sites. No emit. Cost units are relative classes, not time.
//!
//! **Span rendering (U2 decider):** v1 has no multi-file def→path map. Site
//! identity is function id + optional [`DefId`] + statement span. File context
//! is attached from the package entry source when the span falls inside that
//! buffer — not a new package-relative span type. Site `source_line` values
//! are length-capped and interned per unique line so a hostile long line
//! cannot amplify report memory with site count.
//!
//! JSON contract (version 1) — stable key set:
//! - root: `version`, `legend`, `min_class`, `package`, `functions`, `sites`
//! - `package`: `input`, `dominant`, `histogram`, `foldable`, `uncosted`, `hot`, `sites`
//! - `functions[]`: `id`, `name`, `source`, `dominant`, `histogram`, `foldable`, `uncosted`, `hot`
//! - `sites[]`: `function`, `name`, `source`, `block`, `span`, `kind`, `family`, `tag`, `hot`, `element_count`, `file`, `source_line`
//! - `span`: `start`, `end`
//! - `histogram[]`: `class`, `count`

use crate::cli::CostArgs;
use faber_cli::package;
use radix::module::codegen::Target;
use radix::module::diagnostics::Diagnostic;
use radix_mir::{
    CostClass, CostFamily, CostFunctionRollup, CostSite, CostSiteKind, CostTag, cost_rollups,
    cost_sites,
};
use serde::Serialize;
use serde::ser::Serializer;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

/// Legend line present in both human and JSON output.
pub(crate) const COST_LEGEND: &str = "cost units are relative classes, not time";

/// Version of the `--json` machine contract.
pub(crate) const COST_REPORT_VERSION: u32 = 1;

/// Maximum retained bytes of a site `source_line`. Truncated at a UTF-8
/// boundary; sites on the same physical line share one allocation.
pub(crate) const SOURCE_LINE_CAP: usize = 256;

/// Versioned cost report shared by the human renderer and `--json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CostReport {
    pub version: u32,
    pub legend: String,
    pub min_class: String,
    pub package: CostPackageJson,
    pub functions: Vec<CostFunctionJson>,
    pub sites: Vec<CostSiteJson>,
}

/// Package-level roll-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CostPackageJson {
    pub input: String,
    pub dominant: Option<String>,
    pub histogram: Vec<CostHistogramBin>,
    pub foldable: usize,
    pub uncosted: usize,
    pub hot: usize,
    pub sites: usize,
}

/// Per-function roll-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CostFunctionJson {
    pub id: u32,
    pub name: Option<String>,
    pub source: Option<u32>,
    pub dominant: Option<String>,
    pub histogram: Vec<CostHistogramBin>,
    pub foldable: usize,
    pub uncosted: usize,
    pub hot: usize,
}

/// One histogram bucket (`C3×1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CostHistogramBin {
    pub class: String,
    pub count: usize,
}

/// One classified site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CostSiteJson {
    pub function: u32,
    pub name: Option<String>,
    pub source: Option<u32>,
    pub block: u32,
    pub span: CostSpanJson,
    pub kind: String,
    pub family: String,
    pub tag: String,
    pub hot: bool,
    pub element_count: Option<u64>,
    pub file: Option<String>,
    #[serde(serialize_with = "serialize_opt_arc_string")]
    pub source_line: Option<Arc<String>>,
}

fn serialize_opt_arc_string<S: Serializer>(
    value: &Option<Arc<String>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value.as_deref() {
        Some(line) => serializer.serialize_some(line.as_str()),
        None => serializer.serialize_none(),
    }
}

/// Statement span as a half-open byte range — not a path-bearing type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CostSpanJson {
    pub start: u32,
    pub end: u32,
}

#[cfg(feature = "runner")]
pub(super) fn cmd_cost(args: CostArgs) {
    if args.input.is_empty() || args.input[0] == "-" {
        eprintln!("error: package cost requires a path input");
        faber_cli::exit_status::exit_usage();
    }
    let min_class = match parse_min_class(&args.min_class) {
        Ok(class) => class,
        Err(message) => {
            eprintln!("error: {message}");
            faber_cli::exit_status::exit_usage();
        }
    };
    let input = package::absolutize_path(Path::new(&args.input[0]));
    match cost_report_for_input(
        &input,
        args.locale.as_deref(),
        args.diagnostics_locale.as_deref(),
        min_class,
    ) {
        Ok(report) => {
            if args.json {
                match serde_json::to_string_pretty(&report) {
                    Ok(json) => println!("{json}"),
                    Err(err) => {
                        eprintln!("error: failed to render cost JSON: {err}");
                        std::process::exit(1);
                    }
                }
            } else {
                print!("{}", render_human(&report));
            }
        }
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            faber_cli::exit_status::exit_for_diagnostics(&diagnostics);
        }
    }
}

#[cfg(not(feature = "runner"))]
pub(super) fn cmd_cost(_args: CostArgs) {
    super::exit_runner_feature_unavailable("cost");
}

/// Compile a package to validated MIR and build the cost report.
#[cfg(feature = "runner")]
pub(crate) fn cost_report_for_input(
    input: &Path,
    locale: Option<&str>,
    diagnostics_locale: Option<&str>,
    min_class: CostClass,
) -> Result<CostReport, Vec<Diagnostic>> {
    let input_path = package::absolutize_path(input);
    let (config, _locale_pack) =
        match package::config_with_locale(Target::HirRust, &input_path, locale, diagnostics_locale)
        {
            Ok(pair) => pair,
            Err(diag) => {
                let diag = package::attach_path_diagnostic_context(diag);
                return Err(vec![*diag]);
            }
        };
    let (sites, names) = package::with_lowered_package_mir(&config, &input_path, |lowered| {
        let sites = cost_sites(&lowered.validated);
        let mut names = BTreeMap::new();
        for function in &lowered.validated.program().functions {
            if let Some(sym) = function.name {
                names.insert(function.id.0, lowered.interner.resolve(sym).to_owned());
            }
        }
        (sites, names)
    })?;
    let (entry_file, entry_source) = entry_source_context(&input_path);
    Ok(build_report(
        &input_path,
        sites,
        &names,
        min_class,
        entry_file.as_deref(),
        entry_source.as_deref(),
    ))
}

/// Build a report from already-walked sites (command path and tests).
pub(crate) fn build_report(
    input: &Path,
    sites: Vec<CostSite>,
    names: &BTreeMap<u32, String>,
    min_class: CostClass,
    entry_file: Option<&str>,
    entry_source: Option<&str>,
) -> CostReport {
    let mut sites: Vec<_> = sites
        .into_iter()
        .filter(|site| site_visible(site, min_class))
        .collect();
    sort_sites(&mut sites);
    let rollups = cost_rollups(&sites);
    let mut functions: Vec<CostFunctionJson> = rollups
        .iter()
        .map(|rollup| function_json(rollup, &sites, names))
        .collect();
    functions.sort_by(|left, right| {
        class_rank_opt(right.dominant.as_deref())
            .cmp(&class_rank_opt(left.dominant.as_deref()))
            .then(left.name.cmp(&right.name))
            .then(left.id.cmp(&right.id))
    });
    let package = package_json(input, &rollups, sites.len());
    let mut intern = SourceLineInterner::new(entry_file, entry_source);
    let site_views = sites
        .iter()
        .map(|site| site_json(site, names, &mut intern))
        .collect();
    CostReport {
        version: COST_REPORT_VERSION,
        legend: COST_LEGEND.to_owned(),
        min_class: class_name(min_class),
        package,
        functions,
        sites: site_views,
    }
}

/// Human report: legend, sorted sites, per-function roll-ups, package roll-up.
pub(crate) fn render_human(report: &CostReport) -> String {
    let mut out = String::new();
    out.push_str(&report.legend);
    out.push('\n');
    out.push('\n');
    out.push_str("sites:\n");
    if report.sites.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for site in &report.sites {
            out.push_str("  ");
            out.push_str(&render_site_line(site));
            out.push('\n');
        }
    }
    out.push('\n');
    out.push_str("functions:\n");
    if report.functions.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for function in &report.functions {
            let label = function
                .name
                .as_deref()
                .map(strip_controls)
                .unwrap_or_else(|| format!("fn={}", function.id));
            out.push_str(&format!(
                "  {label}: {}\n",
                render_rollup_body(
                    function.dominant.as_deref(),
                    &function.histogram,
                    function.foldable,
                    function.uncosted,
                    function.hot
                )
            ));
        }
    }
    out.push('\n');
    out.push_str(&format!(
        "package: {}\n",
        render_rollup_body(
            report.package.dominant.as_deref(),
            &report.package.histogram,
            report.package.foldable,
            report.package.uncosted,
            report.package.hot
        )
    ));
    out
}

/// Compact JSON (tests pin this form; CLI prints pretty).
#[cfg(test)]
pub(crate) fn render_json_compact(report: &CostReport) -> Result<String, serde_json::Error> {
    serde_json::to_string(report)
}

pub(crate) fn parse_min_class(raw: &str) -> Result<CostClass, String> {
    let normalized = raw.trim();
    let log10 = match normalized {
        "C0" | "c0" | "0" => 0,
        "C1" | "c1" | "1" => 1,
        "C2" | "c2" | "2" => 2,
        "C3" | "c3" | "3" => 3,
        _ => {
            return Err(format!(
                "unknown cost class '{raw}'; expected C0, C1, C2, or C3"
            ));
        }
    };
    Ok(CostClass::from_log10(log10))
}

fn site_visible(site: &CostSite, min_class: CostClass) -> bool {
    match site.tag {
        CostTag::Class(class) => class.log10() >= min_class.log10(),
        CostTag::F | CostTag::U => true,
    }
}

fn sort_sites(sites: &mut [CostSite]) {
    sites.sort_by(|left, right| {
        tag_rank(right.tag)
            .cmp(&tag_rank(left.tag))
            .then(right.hot.cmp(&left.hot))
            .then(left.function.0.cmp(&right.function.0))
            .then(left.span.start.cmp(&right.span.start))
            .then(left.span.end.cmp(&right.span.end))
    });
}

fn tag_rank(tag: CostTag) -> u8 {
    match tag {
        CostTag::Class(class) => 2 + class.log10(),
        CostTag::U => 1,
        CostTag::F => 0,
    }
}

fn class_rank_opt(tag: Option<&str>) -> u8 {
    tag.and_then(|name| name.strip_prefix('C'))
        .and_then(|digits| digits.parse::<u8>().ok())
        .map(|log10| 2 + log10)
        .unwrap_or(0)
}

fn function_json(
    rollup: &CostFunctionRollup,
    sites: &[CostSite],
    names: &BTreeMap<u32, String>,
) -> CostFunctionJson {
    let sample = sites.iter().find(|site| site.function == rollup.function);
    CostFunctionJson {
        id: rollup.function.0,
        name: names.get(&rollup.function.0).cloned(),
        source: sample.and_then(|site| site.source).map(|id| id.0),
        dominant: rollup.dominant.map(class_name),
        histogram: histogram_json(&rollup.class_histogram),
        foldable: rollup.foldable,
        uncosted: rollup.uncosted,
        hot: rollup.hot,
    }
}

fn package_json(
    input: &Path,
    rollups: &[CostFunctionRollup],
    site_count: usize,
) -> CostPackageJson {
    let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
    let mut foldable = 0;
    let mut uncosted = 0;
    let mut hot = 0;
    let mut dominant = None;
    for rollup in rollups {
        foldable += rollup.foldable;
        uncosted += rollup.uncosted;
        hot += rollup.hot;
        if let Some(class) = rollup.dominant {
            dominant = Some(dominant.map_or(class, |best: CostClass| best.max(class)));
        }
        for (class, count) in &rollup.class_histogram {
            *counts.entry(class.log10()).or_insert(0) += *count;
        }
    }
    let histogram: Vec<(CostClass, usize)> = counts
        .into_iter()
        .map(|(log10, count)| (CostClass::from_log10(log10), count))
        .collect();
    CostPackageJson {
        input: input.display().to_string(),
        dominant: dominant.map(class_name),
        histogram: histogram_json(&histogram),
        foldable,
        uncosted,
        hot,
        sites: site_count,
    }
}

fn histogram_json(histogram: &[(CostClass, usize)]) -> Vec<CostHistogramBin> {
    histogram
        .iter()
        .rev()
        .map(|(class, count)| CostHistogramBin {
            class: class_name(*class),
            count: *count,
        })
        .collect()
}

fn site_json(
    site: &CostSite,
    names: &BTreeMap<u32, String>,
    intern: &mut SourceLineInterner<'_>,
) -> CostSiteJson {
    let (file, source_line) = intern.context(site);
    CostSiteJson {
        function: site.function.0,
        name: names.get(&site.function.0).cloned(),
        source: site.source.map(|id| id.0),
        block: site.block.0,
        span: CostSpanJson {
            start: site.span.start,
            end: site.span.end,
        },
        kind: kind_name(site.kind).to_owned(),
        family: family_name(site.family).to_owned(),
        tag: tag_name(site.tag),
        hot: site.hot,
        element_count: site.element_count,
        file,
        source_line,
    }
}

/// Interns capped source lines so sites on one physical line share one copy.
struct SourceLineInterner<'a> {
    file: Option<&'a str>,
    source: Option<&'a str>,
    by_start: HashMap<usize, Arc<String>>,
    ranges: Vec<(usize, usize)>,
}

impl<'a> SourceLineInterner<'a> {
    fn new(file: Option<&'a str>, source: Option<&'a str>) -> Self {
        Self {
            file,
            source,
            by_start: HashMap::new(),
            ranges: Vec::new(),
        }
    }

    fn context(&mut self, site: &CostSite) -> (Option<String>, Option<Arc<String>>) {
        let (Some(file), Some(source)) = (self.file, self.source) else {
            return (None, None);
        };
        let start = site.span.start as usize;
        let end = site.span.end as usize;
        if start > source.len() || end > source.len() {
            return (None, None);
        }
        (Some(file.to_owned()), Some(self.intern_line(source, start)))
    }

    fn intern_line(&mut self, source: &str, offset: usize) -> Arc<String> {
        for &(start, end) in &self.ranges {
            if offset >= start
                && offset <= end
                && let Some(text) = self.by_start.get(&start)
            {
                return text.clone();
            }
        }
        let (start, end) = line_bounds(source, offset);
        let interned = Arc::new(capped_source_line(&source[start..end]).to_owned());
        self.by_start.insert(start, interned.clone());
        self.ranges.push((start, end));
        interned
    }
}

fn line_bounds(source: &str, offset: usize) -> (usize, usize) {
    let offset = snap_char_boundary(source, offset.min(source.len()));
    let start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = source[offset..]
        .find('\n')
        .map_or(source.len(), |i| offset + i);
    (start, end)
}

fn snap_char_boundary(source: &str, mut offset: usize) -> usize {
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn capped_source_line(line: &str) -> &str {
    if line.len() <= SOURCE_LINE_CAP {
        return line;
    }
    let mut end = SOURCE_LINE_CAP;
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}

fn strip_controls(input: &str) -> String {
    input.chars().filter(|ch| !ch.is_control()).collect()
}

fn entry_source_context(input: &Path) -> (Option<String>, Option<String>) {
    let Ok(spec) = package::discover_package(input) else {
        return (None, None);
    };
    let file = spec.entry.display().to_string();
    match std::fs::read_to_string(&spec.entry) {
        Ok(source) => (Some(file), Some(source)),
        Err(_) => (None, None),
    }
}

fn render_site_line(site: &CostSiteJson) -> String {
    let mut line = String::new();
    line.push_str(&site.tag);
    if site.hot {
        line.push_str(" hot");
    }
    line.push(' ');
    line.push_str(&site.kind);
    line.push(' ');
    line.push_str(&site.family);
    if let Some(name) = site.name.as_deref() {
        line.push(' ');
        line.push_str(&strip_controls(name));
    }
    line.push_str(&format!(" fn={}", site.function));
    if let Some(source) = site.source {
        line.push_str(&format!(" def={source}"));
    }
    line.push_str(&format!(" span={}..{}", site.span.start, site.span.end));
    if let Some(count) = site.element_count {
        line.push_str(&format!(" n={count}"));
    }
    if let Some(file) = site.file.as_deref() {
        line.push_str(&format!(" file={}", strip_controls(file)));
    }
    line
}

fn render_rollup_body(
    dominant: Option<&str>,
    histogram: &[CostHistogramBin],
    foldable: usize,
    uncosted: usize,
    hot: usize,
) -> String {
    let mut chunks = Vec::new();
    if let Some(class) = dominant {
        let hist = histogram
            .iter()
            .map(|bin| format!("{}×{}", bin.class, bin.count))
            .collect::<Vec<_>>()
            .join(", ");
        if hist.is_empty() {
            chunks.push(format!("dominant {class}"));
        } else {
            chunks.push(format!("dominant {class} · {hist}"));
        }
    }
    if foldable > 0 {
        chunks.push(format!("F×{foldable}"));
    }
    if uncosted > 0 {
        chunks.push(format!("U×{uncosted}"));
    }
    if hot > 0 {
        chunks.push(format!("hot×{hot}"));
    }
    if chunks.is_empty() {
        "no sites".to_owned()
    } else {
        chunks.join(" · ")
    }
}

fn class_name(class: CostClass) -> String {
    format!("C{}", class.log10())
}

fn tag_name(tag: CostTag) -> String {
    match tag {
        CostTag::F => "F".to_owned(),
        CostTag::U => "U".to_owned(),
        CostTag::Class(class) => class_name(class),
    }
}

fn kind_name(kind: CostSiteKind) -> &'static str {
    match kind {
        CostSiteKind::Convert => "convert",
        CostSiteKind::ExternCall => "extern-call",
    }
}

fn family_name(family: CostFamily) -> &'static str {
    match family {
        CostFamily::Cast => "cast",
        CostFamily::OctetiWindowUnpack => "octeti-window-unpack",
        CostFamily::TextusToNumerus => "textus-to-numerus",
        CostFamily::NumerusToTextus => "numerus-to-textus",
        CostFamily::GenusRecordExtraction => "genus-record-extraction",
        CostFamily::ListTensor => "list-tensor",
        CostFamily::RegisterWindow => "register-window",
        CostFamily::RegisterBridge => "register-bridge",
        CostFamily::RegisterLista => "register-lista",
        CostFamily::TextusToRegex => "textus-to-regex",
        CostFamily::ValorJson => "valor-json",
        CostFamily::ConstantOperand => "constant-operand",
        CostFamily::UnknownExtern => "unknown-extern",
    }
}

/// Test helper: a convert site at `function` with the given tag.
#[cfg(test)]
pub(crate) fn test_site(
    function: u32,
    start: u32,
    tag: CostTag,
    family: CostFamily,
    kind: CostSiteKind,
    hot: bool,
) -> CostSite {
    use radix::module::diagnostics::Span;
    use radix::module::mir::{MirBlockId, MirFunctionId};
    use radix_types::DefId;
    CostSite {
        function: MirFunctionId(function),
        source: Some(DefId(function + 100)),
        name: None,
        block: MirBlockId(0),
        span: Span::new(start, start + 10),
        kind,
        family,
        tag,
        hot,
        element_count: None,
    }
}

#[cfg(test)]
pub(crate) fn test_input_path() -> std::path::PathBuf {
    std::path::PathBuf::from("/tmp/cost-fixture")
}
