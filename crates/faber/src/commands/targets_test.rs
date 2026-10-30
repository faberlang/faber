use super::*;
use crate::cli::FaberCliTarget;
use clap::ValueEnum;

/// The compiled feature set of a hir-rust-only small build.
#[cfg(feature = "hir-rust")]
fn small_compiled() -> BTreeSet<&'static str> {
    ["hir-rust"].into_iter().collect()
}

/// Features of the experimental, default-off novelty targets (BASIC, Pascal,
/// COBOL, Fortran, Odin, C). They are outside `full-targets`, so the default test build
/// lists their rows as `available=no`.
#[cfg(all(feature = "full-targets", feature = "hir-rust"))]
const EXPERIMENTAL_DEFAULT_OFF: [&str; 6] = [
    "mir-basic",
    "hir-pascal",
    "hir-cobol",
    "hir-fortran",
    "hir-odin",
    "hir-c",
];

#[cfg(feature = "hir-rust")]
fn row<'a>(rows: &'a [TargetRow], name: &str) -> &'a TargetRow {
    rows.iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("missing row {name}"))
}

fn row_spec<'a>(specs: &'a [FaberTargetRowSpec], name: &str) -> &'a FaberTargetRowSpec {
    specs
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("missing row spec {name}"))
}

/// The CLI-selectable target universe: every `FaberCliTarget` variant except
/// the deliberately hidden `scena`, `clif-jit` and `runner` spellings.
fn selectable_cli_targets() -> Vec<Target> {
    FaberCliTarget::value_variants()
        .iter()
        .filter(|variant| {
            !matches!(
                variant,
                FaberCliTarget::MirScena | FaberCliTarget::ClifJit | FaberCliTarget::Runner
            )
        })
        .map(|variant| variant.to_radix())
        .collect()
}

/// The primary CLI spelling of each selectable variant, read from the clap
/// value metadata — independent of the row builder's `Target`-`Display`
/// source, so the set-equality test also catches `Display`↔CLI-name drift.
fn selectable_cli_names() -> BTreeSet<String> {
    FaberCliTarget::value_variants()
        .iter()
        .filter(|variant| {
            !matches!(
                variant,
                FaberCliTarget::MirScena | FaberCliTarget::ClifJit | FaberCliTarget::Runner
            )
        })
        .map(|variant| {
            variant
                .to_possible_value()
                .expect("FaberCliTarget variants declare a clap value name")
                .get_name()
                .to_string()
        })
        .collect()
}

#[test]
fn row_set_equals_cli_selectable_variant_set() {
    // S09-3: the derived row set must equal the CLI grammar's own variant
    // set minus the hidden `scena` spelling — a target added to
    // `FaberCliTarget` joins `faber targets` by construction, and a row no
    // CLI spelling selects cannot appear.
    let specs = faber_target_row_specs().expect("every CLI target maps to a faber feature");
    let mut names: BTreeSet<String> = specs.iter().map(|s| s.name.clone()).collect();

    // Documented override (goal Open question 4 ruling): MirWasm keeps two
    // live CLI spellings — primary `wasm-text` and the compatibility alias
    // `wasm` — so the `wasm` alias row stays explicit.
    assert!(
        names.remove("wasm"),
        "the MirWasm `wasm` alias row must stay (documented OQ-4 override)"
    );

    assert_eq!(
        names,
        selectable_cli_names(),
        "row set must equal the FaberCliTarget variant set minus hidden scena"
    );

    // Every row — including the wasm alias — maps to a CLI-selectable target.
    let selectable = selectable_cli_targets();
    for spec in &specs {
        assert!(
            selectable.contains(&spec.target),
            "row {} reports {:?}, which no selectable FaberCliTarget variant selects",
            spec.name,
            spec.target
        );
    }
}

#[test]
fn mirwasm_double_row_shares_target_and_feature() {
    // OQ-4: the `wasm`/`wasm-text` double row stays explicit; both rows must
    // carry identical target and feature truth.
    let specs = faber_target_row_specs().expect("every CLI target maps to a faber feature");
    let wasm = row_spec(&specs, "wasm");
    let wasm_text = row_spec(&specs, "wasm-text");
    assert_eq!(wasm.target, Target::MirWasm);
    assert_eq!(wasm_text.target, Target::MirWasm);
    assert_eq!(wasm.feature, wasm_text.feature);
}

#[test]
fn fmir_family_rows_share_the_fmir_feature_override() {
    // The documented faber-specific override: radix's `required_feature()`
    // leaves the fmir family ungated; faber gates both package-image
    // targets on its `mir-fmir` feature.
    for name in ["fmir-text", "fmir"] {
        let specs = faber_target_row_specs().expect("every CLI target maps to a faber feature");
        let spec = row_spec(&specs, name);
        assert_eq!(
            spec.feature, "mir-fmir",
            "{name} must keep the fmir-family feature override"
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn small_build_reports_only_compiled_capabilities() {
    let rows = target_rows(&small_compiled()).expect("every CLI target maps to a faber feature");

    // The rust host lane is present under hir-rust. `faber run` interprets
    // source and never runs the Rust build, so the lane claims no `run`.
    let rust = row(&rows, "rust");
    assert!(rust.available);
    assert!(
        rust.capabilities.check
            && rust.capabilities.build
            && !rust.capabilities.run
            && rust.capabilities.package,
        "rust host lane must claim check/build/package and no run:\n{}",
        rust.render()
    );

    // Every other derived row is unavailable and claims no capability —
    // derived from the row set itself, so a future CLI variant joins this
    // check by construction.
    for r in &rows {
        if r.name == "rust" {
            continue;
        }
        assert!(
            !r.available,
            "{} must not be available in a small build",
            r.name
        );
        assert!(
            !r.capabilities.build && !r.capabilities.run && !r.capabilities.package,
            "{} must claim no build/run/package capability in a small build:\n{}",
            r.name,
            r.render()
        );
    }

    // No device/host-leaf/device-runtime capability rows at all.
    {
        let name = "device-runtime";
        assert!(
            !rows.iter().any(|r| r.name == name),
            "{name} row must be absent in a small build (capability not compiled)"
        );
    }
}

#[cfg(all(feature = "full-targets", feature = "hir-rust"))]
#[test]
fn default_build_reports_full_surface() {
    // The test binary builds under default features (full-targets), so the
    // real cfg!-derived compiled set is the full emit surface — no
    // table-derived expectation (that circularity hid `swift`).
    let rows = target_rows(&compiled_features()).expect("every CLI target maps to a faber feature");
    let specs = faber_target_row_specs().expect("every CLI target maps to a faber feature");
    // All derived emit rows plus the device-runtime capability row, which is
    // part of the default build (A2 closeout).
    assert_eq!(rows.len(), specs.len() + 1);

    for spec in &specs {
        let available = row(&rows, &spec.name).available;
        if EXPERIMENTAL_DEFAULT_OFF.contains(&spec.feature) {
            assert_eq!(
                available,
                compiled_features().contains(spec.feature),
                "{} availability must follow its default-off feature",
                spec.name
            );
        } else {
            assert!(available, "{} must be available", spec.name);
        }
    }
    {
        let name = "device-runtime";
        assert!(
            rows.iter().any(|r| r.name == name),
            "{name} row must be present in the default build"
        );
    }

    // FMIR rows are faber package build/package targets; `faber run` refuses
    // a built image, so they claim no `run`.
    for name in ["fmir-text", "fmir"] {
        let r = row(&rows, name);
        assert!(
            r.capabilities.check
                && r.capabilities.build
                && !r.capabilities.run
                && r.capabilities.package,
            "faber package FMIR row must show package build truth and no run:\n{}",
            r.render()
        );
        assert!(
            r.capabilities.note.contains("faber build --target"),
            "FMIR row note must point at the faber package build surface:\n{}",
            r.render()
        );
        assert!(
            !r.capabilities.note.contains("radix emit rejects"),
            "FMIR row must not present radix emit truth as faber command truth:\n{}",
            r.render()
        );
    }
}

#[cfg(all(feature = "full-targets", feature = "hir-rust"))]
#[test]
fn rendered_table_contains_capability_rows_under_full_build() {
    let table = rendered_targets_table().expect("every CLI target maps to a faber feature");
    assert!(table.contains("rust available=yes"), "{table}");
    assert!(
        table.contains("leaves=fs,stdio,process,clock,entropy,tcp,http,websocket,machine"),
        "rust must declare every native effect leaf:\n{table}"
    );
    assert!(
        table
            .lines()
            .any(|line| line.starts_with("clif ") && line.contains("leaves=clock")),
        "clif must declare the clock leaf and no other:\n{table}"
    );
    // Capability truth under the default build: device-runtime is compiled,
    // so it reports its row.
    {
        let name = "device-runtime";
        assert!(
            table
                .lines()
                .any(|line| line.starts_with(&format!("{name} "))),
            "missing {name} capability row under the default build:\n{table}"
        );
    }
}

#[cfg(all(feature = "full-targets", feature = "hir-rust"))]
#[test]
fn every_row_feature_is_compiled_in_the_default_test_build() {
    // The test binary builds under default features (full-targets): every
    // row's feature string must name a feature this build actually compiled.
    // Catches a stale `required_feature` mapping, a drifted fmir override,
    // and a `cfg!` arm missing from compiled_features() — the omissions that
    // hid `swift` (P6-2).
    let compiled = compiled_features();
    for spec in faber_target_row_specs().expect("every CLI target maps to a faber feature") {
        if EXPERIMENTAL_DEFAULT_OFF.contains(&spec.feature) {
            continue;
        }
        assert!(
            compiled.contains(spec.feature),
            "row {} names feature {}, which the default \
             (full-targets) build did not compile: stale feature mapping \
             or missing cfg! arm in compiled_features()",
            spec.name,
            spec.feature
        );
    }
}

/// The `run` column and `faber run`'s dispatch agree, both ways.
///
/// `faber run` interprets source and takes no `--target`; a device choice is
/// the only way one of its target routes is reached. So a row is `run=yes`
/// exactly when some `--device` spelling `faber run` accepts launches that
/// row's target, and no row spelling is accepted as `--target`.
#[cfg(all(feature = "full-targets", feature = "hir-rust"))]
#[test]
fn run_column_matches_what_faber_run_dispatches() {
    use crate::cli::{Cli, Command};
    use clap::Parser;

    let rows = target_rows(&compiled_features()).expect("every CLI target maps to a faber feature");
    let specs = faber_target_row_specs().expect("every CLI target maps to a faber feature");

    // Forward: a row that claims `run` is reached by a device spelling that
    // `faber run` parses and that names that row's target.
    for (spec, r) in specs.iter().zip(&rows) {
        assert_eq!(spec.name, r.name, "rows follow the spec order");
        if !r.capabilities.run {
            continue;
        }
        let reaching: Vec<_> = DeviceChoice::value_variants()
            .iter()
            .filter(|choice| choice.run_target_spelling() == Some(spec.name.as_str()))
            .collect();
        if run_target_names().contains(&spec.name) {
            let cli = Cli::try_parse_from(["faber", "run", "--target", &spec.name, "."])
                .unwrap_or_else(|e| panic!("faber run rejects --target {}: {e}", spec.name));
            assert!(matches!(cli.command, Some(Command::Run(_))));
            continue;
        }
        assert!(
            !reaching.is_empty(),
            "{} claims run but neither --device nor --target reaches it",
            r.name
        );
        for choice in reaching {
            let cli = Cli::try_parse_from(["faber", "run", "--device", choice.spelling(), "."])
                .unwrap_or_else(|e| {
                    panic!("faber run rejects --device {}: {e}", choice.spelling())
                });
            let Some(Command::Run(args)) = cli.command else {
                panic!(
                    "`faber run --device {}` did not parse as run",
                    choice.spelling()
                );
            };
            assert_eq!(args.device, Some(*choice));
        }
    }

    // Converse: a row that does not claim `run` is reached by no device
    // choice, and `faber run` accepts no row spelling as a target.
    for (spec, r) in specs.iter().zip(&rows) {
        if r.available && !r.capabilities.run {
            assert!(
                DeviceChoice::value_variants()
                    .iter()
                    .all(|choice| choice.run_target_spelling() != Some(spec.name.as_str())),
                "{} is reached by a --device choice but reports run=no",
                r.name
            );
        }
        assert_eq!(
            Cli::try_parse_from(["faber", "run", "--target", &spec.name, "."]).is_ok(),
            run_target_names().contains(&spec.name),
            "`faber run --target {}` is accepted exactly when the row claims a compiled run",
            spec.name
        );
    }

    // Every device choice that names a target names an emit row.
    for choice in DeviceChoice::value_variants() {
        if let Some(name) = choice.run_target_spelling() {
            assert!(
                specs.iter().any(|spec| spec.name == name),
                "--device {} maps to {name}, which is not a faber targets row",
                choice.spelling()
            );
        }
    }
}
