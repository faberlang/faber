//! Pins for `faber cost` human output and the versioned `--json` contract.

use super::cost::{
    COST_LEGEND, COST_REPORT_VERSION, CostReport, SOURCE_LINE_CAP, build_report, parse_min_class,
    render_human, render_json_compact, test_input_path, test_site,
};
use crate::cli::{Cli, Command};
use clap::{CommandFactory, Parser};
use radix_mir::{CostClass, CostFamily, CostSiteKind, CostTag};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::sync::Arc;

const ROOT_KEYS: [&str; 6] = [
    "version",
    "legend",
    "min_class",
    "package",
    "functions",
    "sites",
];
const PACKAGE_KEYS: [&str; 7] = [
    "input",
    "dominant",
    "histogram",
    "foldable",
    "uncosted",
    "hot",
    "sites",
];
const FUNCTION_KEYS: [&str; 8] = [
    "id",
    "name",
    "source",
    "dominant",
    "histogram",
    "foldable",
    "uncosted",
    "hot",
];
const SITE_KEYS: [&str; 12] = [
    "function",
    "name",
    "source",
    "block",
    "span",
    "kind",
    "family",
    "tag",
    "hot",
    "element_count",
    "file",
    "source_line",
];
const SPAN_KEYS: [&str; 2] = ["start", "end"];
const HISTOGRAM_KEYS: [&str; 2] = ["class", "count"];

/// Compact golden of the synthetic fixture (stable v1 contract).
const SYNTHETIC_COMPACT: &str = r#"{"version":1,"legend":"cost units are relative classes, not time","min_class":"C0","package":{"input":"/tmp/cost-fixture","dominant":"C3","histogram":[{"class":"C3","count":1},{"class":"C1","count":1},{"class":"C0","count":1}],"foldable":1,"uncosted":1,"hot":1,"sites":5},"functions":[{"id":1,"name":"mean_sq","source":101,"dominant":"C3","histogram":[{"class":"C3","count":1},{"class":"C1","count":1},{"class":"C0","count":1}],"foldable":0,"uncosted":0,"hot":1},{"id":2,"name":"folded","source":102,"dominant":null,"histogram":[],"foldable":1,"uncosted":0,"hot":0},{"id":0,"name":"main","source":100,"dominant":null,"histogram":[],"foldable":0,"uncosted":1,"hot":0}],"sites":[{"function":1,"name":"mean_sq","source":101,"block":0,"span":{"start":100,"end":110},"kind":"convert","family":"valor-json","tag":"C3","hot":true,"element_count":null,"file":null,"source_line":null},{"function":1,"name":"mean_sq","source":101,"block":0,"span":{"start":40,"end":50},"kind":"convert","family":"textus-to-numerus","tag":"C1","hot":false,"element_count":null,"file":null,"source_line":null},{"function":1,"name":"mean_sq","source":101,"block":0,"span":{"start":10,"end":20},"kind":"convert","family":"cast","tag":"C0","hot":false,"element_count":null,"file":null,"source_line":null},{"function":0,"name":"main","source":100,"block":0,"span":{"start":200,"end":210},"kind":"extern-call","family":"unknown-extern","tag":"U","hot":false,"element_count":null,"file":null,"source_line":null},{"function":2,"name":"folded","source":102,"block":0,"span":{"start":5,"end":15},"kind":"convert","family":"constant-operand","tag":"F","hot":false,"element_count":null,"file":null,"source_line":null}]}"#;

fn names() -> BTreeMap<u32, String> {
    let mut map = BTreeMap::new();
    map.insert(0, "main".to_owned());
    map.insert(1, "mean_sq".to_owned());
    map.insert(2, "folded".to_owned());
    map
}

fn synthetic_sites() -> Vec<radix_mir::CostSite> {
    vec![
        test_site(
            1,
            100,
            CostTag::Class(CostClass::C3),
            CostFamily::ValorJson,
            CostSiteKind::Convert,
            true,
        ),
        test_site(
            1,
            40,
            CostTag::Class(CostClass::C1),
            CostFamily::TextusToNumerus,
            CostSiteKind::Convert,
            false,
        ),
        test_site(
            1,
            10,
            CostTag::Class(CostClass::C0),
            CostFamily::Cast,
            CostSiteKind::Convert,
            false,
        ),
        test_site(
            0,
            200,
            CostTag::U,
            CostFamily::UnknownExtern,
            CostSiteKind::ExternCall,
            false,
        ),
        test_site(
            2,
            5,
            CostTag::F,
            CostFamily::ConstantOperand,
            CostSiteKind::Convert,
            false,
        ),
    ]
}

fn synthetic_report(min_class: CostClass) -> CostReport {
    build_report(
        &test_input_path(),
        synthetic_sites(),
        &names(),
        min_class,
        None,
        None,
    )
}

fn object_keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn assert_key_set(value: &Value, expected: &[&str]) {
    let keys = object_keys(value);
    let want: BTreeSet<&str> = expected.iter().copied().collect();
    assert_eq!(keys, want, "key set mismatch for {value}");
}

fn write_cost_fixture(root: &Path) -> std::path::PathBuf {
    let package = root.join("pkg");
    std::fs::create_dir_all(package.join("src")).expect("create cost fixture source");
    std::fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "cost-fixture"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    std::fs::write(
        package.join("src/main.fab"),
        r#"
fn as_float(int n) → float {
    return n ↦ float
}

fn parse_int(string s) → int ⇥ string {
    return s ↦ int
}

fn compile_re(string s) → regex ⇥ string {
    return s ↦ regex
}

fn encode(value v) → json ⇥ string {
    return v ↦ json
}

fn as_flag(string s) → bool {
    return s ↦ bool
}

fn folded() → int ⇥ string {
    return "42" ↦ int
}

fn hot_cast(int n) → float {
    mut _ i ← 0
    mut _ acc ← 0.0
    while i ≺ n {
        acc ← acc + (n ↦ float)
        i ← i + 1
    }
    return acc
}

main {
    do {
        let float a ← as_float(1)
        let int b ← parse_int("7")
        let regex r ← compile_re("x")
        let json j ← encode({ "k": 1 } ↦ value)
        let bool flag ← as_flag("x")
        let int f ← folded()
        let float h ← hot_cast(2)
        print a
        print b
        print r
        print j
        print flag
        print f
        print h
    }
    catch err {
        print err
    }
}
"#,
    )
    .expect("write source");
    package
}

fn bin_count(report: &CostReport, class: &str) -> usize {
    report
        .package
        .histogram
        .iter()
        .find(|bin| bin.class == class)
        .map(|bin| bin.count)
        .unwrap_or(0)
}

fn tag_count(report: &CostReport, tag: &str) -> usize {
    report.sites.iter().filter(|site| site.tag == tag).count()
}

#[test]
fn human_report_matches_goal_rollup_shape_and_legend() {
    let report = synthetic_report(CostClass::C0);
    let human = render_human(&report);
    assert!(
        human.starts_with(COST_LEGEND),
        "legend must lead the human report:\n{human}"
    );
    assert!(
        human.contains("mean_sq: dominant C3 · C3×1, C1×1, C0×1"),
        "function roll-up must match the goal shape:\n{human}"
    );
    assert!(
        human.contains("package: dominant C3 · C3×1, C1×1, C0×1 · F×1 · U×1 · hot×1"),
        "package roll-up must carry dominant class, histogram, and F/U/hot:\n{human}"
    );
    assert!(
        human.contains("C3 hot convert valor-json mean_sq fn=1 def=101 span=100..110"),
        "sites sort class-desc then hot-before-cold:\n{human}"
    );
    let c3 = human.find("C3 hot convert").expect("C3 site");
    let c0 = human.find("C0 convert").expect("C0 site");
    let u = human.find("U extern-call").expect("U site");
    let f = human.find("F convert").expect("F site");
    assert!(c3 < c0 && c0 < u && u < f, "sort order:\n{human}");
}

#[test]
fn json_contract_parses_with_stable_key_set_and_version() {
    let report = synthetic_report(CostClass::C0);
    let compact = render_json_compact(&report).expect("compact json");
    assert_eq!(compact, SYNTHETIC_COMPACT);
    let value: Value = serde_json::from_str(&compact).expect("parse json");
    assert_eq!(value["version"], COST_REPORT_VERSION);
    assert_eq!(value["legend"], COST_LEGEND);
    assert_eq!(value["min_class"], "C0");
    assert_key_set(&value, &ROOT_KEYS);
    assert_key_set(&value["package"], &PACKAGE_KEYS);
    assert_key_set(&value["functions"][0], &FUNCTION_KEYS);
    assert_key_set(&value["sites"][0], &SITE_KEYS);
    assert_key_set(&value["sites"][0]["span"], &SPAN_KEYS);
    assert_key_set(&value["package"]["histogram"][0], &HISTOGRAM_KEYS);
    assert!(
        compact.contains(COST_LEGEND),
        "legend must be present in JSON"
    );
}

#[test]
fn min_class_c1_drops_c0_keeps_side_buckets() {
    let report = synthetic_report(CostClass::C1);
    assert_eq!(report.min_class, "C1");
    assert_eq!(report.package.histogram.len(), 2);
    assert_eq!(report.package.histogram[0].class, "C3");
    assert_eq!(report.package.histogram[1].class, "C1");
    assert_eq!(report.package.foldable, 1);
    assert_eq!(report.package.uncosted, 1);
    assert!(report.sites.iter().all(|site| site.tag != "C0"));
    assert!(report.sites.iter().any(|site| site.tag == "F"));
    assert!(report.sites.iter().any(|site| site.tag == "U"));
    let human = render_human(&report);
    assert!(human.contains(COST_LEGEND));
    assert!(!human.contains("C0 convert"), "C0 filtered:\n{human}");
}

#[test]
fn parse_min_class_accepts_c_forms() {
    assert_eq!(parse_min_class("C0").unwrap(), CostClass::C0);
    assert_eq!(parse_min_class("c1").unwrap(), CostClass::C1);
    assert_eq!(parse_min_class("2").unwrap(), CostClass::C2);
    assert!(parse_min_class("C9").is_err());
}

#[test]
fn cli_parses_cost_json_and_min_class() {
    let cli = Cli::try_parse_from(["faber", "cost", "--json", "--min-class", "C1", "."])
        .expect("parse cost");
    let Some(Command::Cost(args)) = cli.command else {
        panic!("expected cost subcommand");
    };
    assert!(args.json);
    assert_eq!(args.min_class, "C1");
    assert_eq!(args.input, vec![".".to_owned()]);
}

#[test]
fn cli_help_documents_json_min_class_and_legend() {
    let mut command = Cli::command();
    let cost = command
        .find_subcommand_mut("cost")
        .expect("cost subcommand");
    let help = cost.render_long_help().to_string();
    assert!(help.contains("--json"), "{help}");
    assert!(help.contains("--min-class"), "{help}");
    assert!(help.contains("relative classes, not time"), "{help}");
}

#[test]
fn report_over_fixture_package_pins_dominant_classes_and_buckets() {
    let root = tempfile::tempdir().expect("cost fixture root");
    let package = write_cost_fixture(root.path());
    let report = super::cost::cost_report_for_input(&package, None, None, CostClass::C0)
        .unwrap_or_else(|diagnostics| panic!("cost fixture must lower: {:?}", diagnostics));
    assert_eq!(report.version, COST_REPORT_VERSION);
    assert_eq!(report.legend, COST_LEGEND);
    assert_eq!(report.package.dominant.as_deref(), Some("C3"));
    assert!(
        bin_count(&report, "C3") >= 1,
        "C3 valor↦json must appear: {report:?}"
    );
    assert!(
        bin_count(&report, "C2") >= 1,
        "C2 textus↦regex must appear: {report:?}"
    );
    assert!(
        bin_count(&report, "C1") >= 1,
        "C1 textus↦numerus must appear: {report:?}"
    );
    assert!(
        bin_count(&report, "C0") >= 1,
        "C0 default inclusion: {report:?}"
    );
    assert!(
        report.package.foldable >= 1,
        "F bucket from constant operand: {report:?}"
    );
    assert!(
        report.package.uncosted >= 1,
        "U bucket from unmatched conversion: {report:?}"
    );
    assert!(
        report.package.hot >= 1,
        "hot tag from loop-member convert: {report:?}"
    );

    let filtered = super::cost::cost_report_for_input(&package, None, None, CostClass::C1)
        .expect("filtered cost report");
    assert_eq!(tag_count(&filtered, "C0"), 0);
    assert!(tag_count(&filtered, "C1") >= 1);
    assert!(tag_count(&filtered, "F") >= 1);
    assert!(tag_count(&filtered, "U") >= 1);
}

fn cost_faber_binary() -> std::path::PathBuf {
    super::format_test::faber_binary()
}

#[test]
fn cli_cost_json_and_min_class_on_fixture() {
    let root = tempfile::tempdir().expect("cost cli fixture root");
    let package = write_cost_fixture(root.path());
    let binary = cost_faber_binary();
    assert!(
        binary.is_file(),
        "faber binary must exist at {} (build -p faber --bin faber)",
        binary.display()
    );

    let json_output = ProcessCommand::new(&binary)
        .args(["cost", "--json"])
        .arg(&package)
        .output()
        .expect("run faber cost --json");
    let stdout = String::from_utf8_lossy(&json_output.stdout);
    let stderr = String::from_utf8_lossy(&json_output.stderr);
    assert!(
        json_output.status.success(),
        "faber cost --json must succeed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    let value: Value = serde_json::from_str(stdout.trim()).expect("cli json parses");
    assert_eq!(value["version"], COST_REPORT_VERSION);
    assert_eq!(value["legend"], COST_LEGEND);
    assert_key_set(&value, &ROOT_KEYS);
    assert_eq!(value["package"]["dominant"], "C3");

    let filtered = ProcessCommand::new(&binary)
        .args(["cost", "--min-class", "C1"])
        .arg(&package)
        .output()
        .expect("run faber cost --min-class");
    let human = String::from_utf8_lossy(&filtered.stdout);
    let filtered_err = String::from_utf8_lossy(&filtered.stderr);
    assert!(
        filtered.status.success(),
        "faber cost --min-class C1 must succeed:\nstdout: {human}\nstderr: {filtered_err}"
    );
    assert!(human.contains(COST_LEGEND), "{human}");
    assert!(
        !human
            .lines()
            .any(|line| line.contains(" C0 ") || line.starts_with("  C0 ")),
        "filtered human report must drop C0 sites:\n{human}"
    );
}

fn unique_interned_source_line_bytes(report: &CostReport) -> usize {
    let mut seen = HashSet::new();
    let mut bytes = 0usize;
    for site in &report.sites {
        if let Some(line) = &site.source_line {
            let ptr = Arc::as_ptr(line) as *const u8;
            if seen.insert(ptr) {
                bytes += line.len();
            }
        }
    }
    bytes
}

#[test]
fn source_lines_are_capped_and_shared_across_sites_on_the_same_line() {
    const LINE_LEN: usize = 64 * 1024;
    const SITE_COUNT: usize = 1_000;
    let line = "a".repeat(LINE_LEN);
    let sites: Vec<_> = (0..SITE_COUNT)
        .map(|i| {
            test_site(
                1,
                (i * 10) as u32,
                CostTag::Class(CostClass::C0),
                CostFamily::Cast,
                CostSiteKind::Convert,
                false,
            )
        })
        .collect();
    let names = BTreeMap::new();
    let report = build_report(
        &test_input_path(),
        sites,
        &names,
        CostClass::C0,
        Some("main.fab"),
        Some(&line),
    );
    assert_eq!(report.sites.len(), SITE_COUNT);
    let first = report.sites[0]
        .source_line
        .as_ref()
        .expect("source_line attached");
    assert!(
        first.len() <= SOURCE_LINE_CAP,
        "capped length {}, cap {SOURCE_LINE_CAP}",
        first.len()
    );
    assert_eq!(&line[..SOURCE_LINE_CAP], first.as_ref());
    for site in &report.sites {
        let shared = site.source_line.as_ref().expect("source_line attached");
        assert!(
            Arc::ptr_eq(first, shared),
            "sites on one line must share one source_line allocation"
        );
        assert!(shared.len() <= SOURCE_LINE_CAP);
    }
    let retained = unique_interned_source_line_bytes(&report);
    assert!(
        retained <= SOURCE_LINE_CAP,
        "interned source_line bytes {retained} must stay at the cap, not SITE_COUNT × LINE_LEN = {}",
        SITE_COUNT * LINE_LEN
    );
}

#[test]
fn human_renderer_strips_control_bytes_json_preserves_them() {
    let mut names = BTreeMap::new();
    names.insert(1, "mean\x1b[31msq".to_owned());
    let source = "x".repeat(200);
    let file = "src/\x1b[31mmain.fab";
    let report = build_report(
        &test_input_path(),
        vec![test_site(
            1,
            100,
            CostTag::Class(CostClass::C3),
            CostFamily::ValorJson,
            CostSiteKind::Convert,
            true,
        )],
        &names,
        CostClass::C0,
        Some(file),
        Some(&source),
    );
    let human = render_human(&report);
    assert!(
        !human.contains('\u{1b}'),
        "human output must not carry ESC:\n{human:?}"
    );
    assert!(
        human.contains("mean[31msq"),
        "stripped name must render as inert text:\n{human}"
    );
    assert!(
        human.contains("file=src/[31mmain.fab"),
        "stripped file must render as inert text:\n{human}"
    );
    let compact = render_json_compact(&report).expect("json");
    let value: Value = serde_json::from_str(&compact).expect("parse json");
    assert_eq!(value["sites"][0]["name"], "mean\u{1b}[31msq");
    assert_eq!(value["sites"][0]["file"], "src/\u{1b}[31mmain.fab");
    assert_eq!(value["functions"][0]["name"], "mean\u{1b}[31msq");
}
