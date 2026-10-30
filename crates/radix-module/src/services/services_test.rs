//! Tests for the tool subcommand system (compile, check, emit, inspect,
//! etc.).
//! Proves: tool dispatch; source compilation through the tool entry point;
//! reader-pack integration; target discovery and capability reporting.

use super::*;
use crate::driver::AnalyzeRequest;
use crate::program::{compile_cli_path, compile_cli_source, compile_cli_source_with_locale_pack};
use crate::services::{
    TargetCommandSurface, check_diagnostics_for_source, check_diagnostics_for_source_with_locale_pack,
    discoverable_targets, target_capabilities, target_capabilities_for_surface, target_extension, target_leaves,
    target_name, verify_source,
};
use std::path::PathBuf;

fn temp_fab(label: &str, content: &str) -> (tempfile::TempDir, String) {
    let dir = crate::test_support::test_temp_dir(&format!("main-{label}"));
    let path = dir.path().join(format!("{label}.fab"));
    std::fs::write(&path, content).expect("write temp fab");
    (dir, path.to_string_lossy().to_string())
}

/// Tag a Latin test source with its `la` reader locale.
fn la(source: &str) -> String {
    format!("+++\nlocale = \"la\"\n+++\n{source}")
}

fn temp_dir(label: &str) -> tempfile::TempDir {
    crate::test_support::test_temp_dir(&format!("main-dir-{label}"))
}

fn zh_locale_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate")
}

/// Path to the triga-free shader-contract source fixture (U4).
fn shader_contract_source_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/shader-contract-source-facts.fab")
}

/// Compile the triga-free shader-contract fixture through the CLI WGSL path.
///
/// The driver ingests the fixture's `@ shader_contract "vertex_layout"`
/// contract calls by resolved identity (U2) and emits the generic source
/// vertex entry — no `triga:` imports, no live-sibling checkout resolution.
fn compile_shader_contract_source_fixture() -> crate::CompileResult {
    compile_cli_path(&shader_contract_source_fixture_path(), false, crate::codegen::Target::MirWgsl)
}

#[cfg(feature = "mir-wgsl")]
fn wgsl_output_reflection(result: crate::CompileResult) -> (String, crate::mir::MirGpuReflection) {
    let output = result.output.expect("wgsl output");
    let crate::Output::WgslText(wgsl_output) = &output else {
        panic!("expected WGSL output");
    };
    let reflection = wgsl_output
        .reflection
        .as_ref()
        .expect("WGSL reflection")
        .clone();
    (wgsl_output.code.clone(), reflection)
}

const OMNIA_EXEMPLUM: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../corpus/omnia/omnia.fab"));

fn assert_json_object_keys(value: &serde_json::Value, expected: &[&str]) {
    let object = value.as_object().expect("json object");
    let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    actual.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected, "json object keys");
}

#[cfg(all(feature = "hir-rust", feature = "hir-ts"))]
#[test]
fn cmd_targets_rust_scena_typescript_capabilities() {
    let rust = target_capabilities(crate::codegen::Target::HirRust);
    assert!(rust.package);
    assert!(rust.build);

    let scena = target_capabilities(crate::codegen::Target::MirScena);
    assert!(!scena.package);
    assert!(!scena.build);
    assert!(!scena.run);
    assert!(
        scena
            .note
            .contains("delegated to `faber build --target scena`"),
        "unexpected scena note: {}",
        scena.note
    );
    assert_eq!(target_name(crate::codegen::Target::MirScena), "scena");
    assert_eq!(target_extension(crate::codegen::Target::MirScena), "scena");

    let ts = target_capabilities(crate::codegen::Target::HirTypeScript);
    assert!(!ts.package);

    assert_eq!(target_extension(crate::codegen::Target::HirGo), "go");
    assert_eq!(target_name(crate::codegen::Target::HirPython), "python");
    assert_eq!(target_extension(crate::codegen::Target::HirPython), "py");
    let python = target_capabilities(crate::codegen::Target::HirPython);
    assert!(python.check);
    assert!(python.build);
    assert!(!python.run);
    assert!(!python.package);
}

#[test]
fn cmd_targets_fmir_delegated_capabilities() {
    let fmir_text = target_capabilities(crate::codegen::Target::MirFmir);
    assert!(!fmir_text.package);
    assert!(!fmir_text.build);
    assert!(!fmir_text.run);
    assert!(
        fmir_text
            .note
            .contains("delegated to `faber build --target fmir-text`"),
        "unexpected fmir-text note: {}",
        fmir_text.note
    );
    assert_eq!(target_name(crate::codegen::Target::MirFmir), "fmir-text");
    assert_eq!(target_extension(crate::codegen::Target::MirFmir), "fmir.txt");

    let fmir = target_capabilities(crate::codegen::Target::MirFmirBinary);
    assert!(!fmir.package);
    assert!(!fmir.build);
    assert!(!fmir.run);
    assert!(
        fmir.note
            .contains("delegated to `faber build --target fmir`"),
        "unexpected fmir note: {}",
        fmir.note
    );
    assert_eq!(target_name(crate::codegen::Target::MirFmirBinary), "fmir");
    assert_eq!(target_extension(crate::codegen::Target::MirFmirBinary), "fmir");
}

#[test]
fn cmd_targets_wasm_family_capabilities() {
    let target = crate::codegen::Target::MirWasm;
    let caps = target_capabilities(target);
    assert!(!caps.package);
    assert!(!caps.run);
    assert!(
        caps.note.contains("supported MIR-backed") && caps.note.contains("faber_* imports"),
        "unexpected {} note: {}",
        target_name(target),
        caps.note
    );
    assert!(
        !caps.note.contains("experimental") && !caps.note.contains("probe"),
        "{} note must not use experimental/probe wording: {}",
        target_name(target),
        caps.note
    );
    assert_eq!(target_name(crate::codegen::Target::MirWasm), "wasm-text");
    assert_eq!(target_extension(crate::codegen::Target::MirWasm), "wat");
}

#[test]
fn cmd_targets_mir_backed_targets_use_supported_wording() {
    let sexp = target_capabilities(crate::codegen::Target::MirSexp);
    assert!(!sexp.package);
    assert!(!sexp.run);
    assert!(
        sexp.note.contains("supported MIR-backed Racket validation"),
        "unexpected sexp note: {}",
        sexp.note
    );
    assert!(
        !sexp.note.contains("experimental") && !sexp.note.contains("probe"),
        "sexp note must not use experimental/probe wording: {}",
        sexp.note
    );

    let llvm = target_capabilities(crate::codegen::Target::MirLlvm);
    assert!(!llvm.package);
    assert!(!llvm.run);
    assert!(
        llvm.note.contains("supported MIR-backed LLVM IR staging"),
        "unexpected llvm-text note: {}",
        llvm.note
    );
    assert!(
        !llvm.note.contains("experimental") && !llvm.note.contains("probe"),
        "llvm-text note must not use experimental/probe wording: {}",
        llvm.note
    );
    assert_eq!(target_name(crate::codegen::Target::MirLlvm), "llvm-text");
    assert_eq!(target_extension(crate::codegen::Target::MirLlvm), "ll");

    let metal = target_capabilities(crate::codegen::Target::MirMetal);
    assert!(!metal.package);
    assert!(!metal.run);
    assert!(
        metal
            .note
            .contains("supported MIR-backed Metal compute shader source"),
        "unexpected metal-text note: {}",
        metal.note
    );
    assert!(
        !metal.note.contains("experimental") && !metal.note.contains("probe"),
        "metal-text note must not use experimental/probe wording: {}",
        metal.note
    );

    let wgsl = target_capabilities(crate::codegen::Target::MirWgsl);
    assert!(!wgsl.package);
    assert!(!wgsl.run);
    assert!(
        wgsl.note
            .contains("supported MIR-backed WGSL compute shader source")
            && wgsl.note.contains("naga"),
        "unexpected wgsl-text note: {}",
        wgsl.note
    );
    assert!(
        !wgsl.note.contains("experimental") && !wgsl.note.contains("probe"),
        "wgsl-text note must not use experimental/probe wording: {}",
        wgsl.note
    );
}

#[test]
fn faber_surface_fhir_row_is_package_capable_and_not_run() {
    // Stage 4: `faber targets` distinguishes FHIR package build from the radix
    // unit-artifact surface (which stays run=no package=no). `faber run` does
    // not load an envelope, so the Faber surface carries no `run` either.
    let faber = target_capabilities_for_surface(TargetCommandSurface::Faber, crate::codegen::Target::HirFhir);
    assert!(faber.check);
    assert!(faber.build);
    assert!(!faber.run);
    assert!(faber.package);
    assert!(
        faber.note.contains("faber build --target fhir"),
        "unexpected Faber fhir note: {}",
        faber.note
    );
}

#[cfg(feature = "mir-llvm")]
#[test]
fn cmd_targets_llvm_host_and_llvm_text_keep_separate_rows() {
    // The emit-only llvm-text row keeps run/package off the radix surface;
    // llvm-host is the product target whose Faber row builds and packages;
    // `faber run` takes no target, so no row here claims `run`.
    let text_radix = target_capabilities(crate::codegen::Target::MirLlvm);
    assert!(text_radix.check);
    assert!(text_radix.build);
    assert!(!text_radix.run);
    assert!(!text_radix.package);

    let host_radix = target_capabilities(crate::codegen::Target::MirLlvmHost);
    assert!(host_radix.check);
    assert!(!host_radix.build);
    assert!(!host_radix.run);
    assert!(!host_radix.package);

    let host_faber = target_capabilities_for_surface(TargetCommandSurface::Faber, crate::codegen::Target::MirLlvmHost);
    assert!(host_faber.build);
    assert!(!host_faber.run);
    assert!(host_faber.package);
    assert!(
        !host_faber.note.contains("build/run --target"),
        "llvm-host note must not advertise a `faber run` target: {}",
        host_faber.note
    );
}

#[test]
fn cmd_targets_discovery_includes_llvm_host() {
    let names = discoverable_targets()
        .iter()
        .copied()
        .map(target_name)
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"llvm-host"),
        "llvm-host must appear in discoverable targets: {names:?}"
    );
    assert_eq!(
        names.iter().filter(|name| **name == "llvm-host").count(),
        1,
        "llvm-host must appear exactly once (no alias spelling): {names:?}"
    );
}

#[test]
fn cmd_targets_keeps_faber_package_capabilities_off_radix_surface() {
    for (target, delegated_note) in [
        (crate::codegen::Target::MirFmir, "delegated to `faber build --target fmir-text`"),
        (
            crate::codegen::Target::MirFmirBinary,
            "delegated to `faber build --target fmir`",
        ),
    ] {
        let radix = target_capabilities_for_surface(TargetCommandSurface::Radix, target);
        let faber = target_capabilities_for_surface(TargetCommandSurface::Faber, target);

        assert!(radix.check);
        assert!(!radix.build);
        assert!(!radix.run);
        assert!(!radix.package);
        assert!(radix.note.contains(delegated_note), "unexpected Radix note: {}", radix.note);
        assert!(faber.build);
        assert!(!faber.run);
        assert!(faber.package);
        assert!(
            faber.note.contains("via `faber build --target"),
            "unexpected Faber note: {}",
            faber.note
        );
    }
}

#[test]
fn cmd_targets_discovery_hides_legacy_scena() {
    let names = discoverable_targets()
        .iter()
        .copied()
        .map(target_name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"fmir-text"));
    assert!(names.contains(&"fmir"));
    assert!(!names.contains(&"scena"), "scena must stay hidden from target discovery");
}

#[test]
fn discoverable_targets_equal_universe_minus_hidden() {
    // P6-3: the discoverable row list is a selection of the variant
    // universe, never an independent enumeration. The delta must be
    // exactly the deliberately hidden variants — MirAmd (internal until
    // the leaf proves a truthful baseline, OQ2), MirScena (hidden legacy
    // runtime), and MirStepper (no CLI spelling exists; S10-12 removed it
    // from discovery, its capability note keeps steering users at
    // `faber script`/`faber test`). A variant added to `Target::ALL` fails
    // this test until it joins the row list or is named hidden here: a new
    // backend can no longer silently vanish from `radix targets` /
    // `faber targets`.
    let discoverable = discoverable_targets();
    let universe_minus_hidden: Vec<crate::codegen::Target> = crate::codegen::Target::ALL
        .iter()
        .copied()
        .filter(|target| {
            !matches!(
                target,
                crate::codegen::Target::MirAmd | crate::codegen::Target::MirScena | crate::codegen::Target::MirStepper
            )
        })
        .collect();

    for target in &universe_minus_hidden {
        assert!(
            discoverable.contains(target),
            "{target:?} is in the target universe and not hidden, but has no discoverable row"
        );
    }
    for target in discoverable {
        assert!(
            universe_minus_hidden.contains(target),
            "{target:?} has a discoverable row but is hidden or absent from the target universe"
        );
    }
    assert_eq!(
        discoverable.len(),
        universe_minus_hidden.len(),
        "discoverable rows must not hold duplicates"
    );
}

#[test]
fn target_name_table_matches_display() {
    // Service names and Display must share the target-owned canonical spelling,
    // including hidden targets without a discovery row.
    for target in crate::codegen::Target::ALL {
        assert_eq!(
            target_name(*target),
            target.to_string(),
            "target_name must spell {target:?} exactly as Display does"
        );
    }
}

#[test]
fn rust_runner_and_go_declare_the_n2_leaves() {
    use crate::codegen::Leaf;
    for target in [
        crate::codegen::Target::HirRust,
        crate::codegen::Target::MirFmir,
        crate::codegen::Target::MirFmirBinary,
        crate::codegen::Target::HirGo,
    ] {
        assert_eq!(target_leaves(target), Leaf::ALL, "{target} must declare N2 leaves");
    }
}

#[test]
fn cmd_targets_reports_known_capabilities() {
    // U5.1 done_when §6: fhir is among known/discoverable targets with correct F8 values.
    let names: Vec<&str> = discoverable_targets()
        .iter()
        .copied()
        .map(target_name)
        .collect();
    assert!(names.contains(&"fhir"), "fhir must be in discoverable targets; got: {names:?}");

    let caps = target_capabilities(crate::codegen::Target::HirFhir);
    assert!(caps.check, "fhir check must be true");
    assert!(caps.build, "fhir build must be true");
    assert!(!caps.run, "fhir run must be false");
    assert!(!caps.package, "fhir package must be false");
}

#[test]
fn target_rows_gate_workflows_on_compiled_backend_availability() {
    for target in [
        crate::codegen::Target::HirRust,
        crate::codegen::Target::HirTypeScript,
        crate::codegen::Target::HirGo,
        crate::codegen::Target::HirSwift,
        crate::codegen::Target::HirPython,
        crate::codegen::Target::HirFaber,
        crate::codegen::Target::HirFhir,
        crate::codegen::Target::MirWasm,
        crate::codegen::Target::MirMetal,
        crate::codegen::Target::MirWgsl,
        crate::codegen::Target::MirSexp,
    ] {
        for surface in [TargetCommandSurface::Radix, TargetCommandSurface::Faber] {
            let row = target_capabilities_for_surface(surface, target);
            if !target.is_available() {
                assert!(!row.build, "{surface:?} {target:?} cannot build when unavailable");
                assert!(!row.run, "{surface:?} {target:?} cannot run when unavailable");
                assert!(!row.package, "{surface:?} {target:?} cannot package when unavailable");
            }
        }
    }
}

#[test]
fn amd_target_has_no_discoverable_row_and_gates_workflows() {
    // AMD-A2 done_when (d): the amd target is the OQ2 INTERNAL variant —
    // NO discoverable `radix targets` row until the leaf proves a truthful
    // baseline (deferred target-support-matrix posture). The capability
    // row exists for exhaustive routing but is gated by `mir-amd`
    // availability like every other feature-gated target.
    assert!(
        !discoverable_targets().contains(&crate::codegen::Target::MirAmd),
        "the amd target must not appear in the discoverable target rows (OQ2)"
    );
    let row = target_capabilities(crate::codegen::Target::MirAmd);
    assert!(row.check, "amd check capability is target-neutral");
    if !crate::codegen::Target::MirAmd.is_available() {
        assert!(!row.build, "amd cannot build when mir-amd is not compiled in");
        assert!(!row.run, "amd cannot run when mir-amd is not compiled in");
        assert!(!row.package, "amd cannot package when mir-amd is not compiled in");
    }
}

#[test]
fn stepper_target_has_no_discoverable_row_and_keeps_note() {
    // S10-12 (goal Open question 2 ruling): no CLI surface spells
    // `MirStepper`, so it is not a discoverable `radix targets` row — the
    // report must not overstate the selectable set. The capability note is
    // the surviving steer: the real entry points are `faber script` /
    // `faber test`, and the `radix targets` printer surfaces this note as
    // a footnote under the row list.
    assert!(
        !discoverable_targets().contains(&crate::codegen::Target::MirStepper),
        "the stepper target has no CLI spelling and must not appear in the discoverable target rows (S10-12)"
    );
    let row = target_capabilities(crate::codegen::Target::MirStepper);
    assert!(
        row.note.contains("faber script") && row.note.contains("faber test"),
        "the hidden stepper row must keep steering users at faber script/faber test: {}",
        row.note
    );
}

#[test]
fn cmd_targets_reports_fhir() {
    // Target::HirFhir capability row is visible in the targets registry.
    let caps = target_capabilities(crate::codegen::Target::HirFhir);
    assert!(!caps.package);
    assert!(!caps.run);
    assert!(caps.check, "fhir check must be true");
    assert!(caps.build, "fhir build must be true");
    assert!(
        caps.note.contains("portable analyzed HIR artifact"),
        "unexpected fhir note: {}",
        caps.note
    );
}

#[test]
fn hir_aspect_verify_reports_checker_unavailable_without_success() {
    let result = verify_source("omnia.fab", OMNIA_EXEMPLUM, None);

    assert!(!result.success);
    let coverage = result.coverage.expect("coverage report");
    let rendered = coverage.render_text();
    assert!(rendered.contains("abstracted"));
    assert!(rendered.contains("checker unavailable: no Lean checker was configured"));
}

#[test]
fn diagnostics_check_reports_parse_record() {
    let result = check_diagnostics_for_source("main.fab", &la("si verum tacet"), false);

    assert!(!result.success);
    assert_eq!(result.diagnostics.len(), 1);
    let output = crate::diagnostics::render_expanded(&result.diagnostics[0]);

    // ALLOW_DIAGNOSTIC_RENDER_TEXT: tool output rendered diagnostic contract

    assert!(output.contains("error[PARSE022] parse main.fab"));
    assert!(output.contains("phase: parse"));
    assert!(output.contains("file: main.fab"));
    assert!(output.contains("span: "));
    assert!(output.contains("source: \u{2068}si verum tacet\u{2069}"));
    assert!(!output.contains("help: "));
}

#[test]
fn diagnostics_check_accepts_frontmatter_on_proba_source() {
    // The corpus fixture is a real frontmatter-bearing source; the `.proba`
    // name exercises the adjacent test-manifest check surface.
    let source = include_str!("../../../../corpus/proba/proba.fab");
    let result = check_diagnostics_for_source("corpus/proba/proba.proba", source, false);

    assert!(
        result.success,
        "frontmatter must be peeled before checking .proba: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error())
    );
}

#[test]
fn diagnostics_check_reports_semantic_record() {
    let result = check_diagnostics_for_source("main.fab", "missing", false);

    assert!(!result.success);
    // em-u2 complete set: the resolve-phase SEM001 (unknown_identifier) and
    // the lowering-phase SEM008 (undefined_name) co-report for the same use.
    assert_eq!(result.diagnostics.len(), 2);
    let output = crate::diagnostics::render_expanded(&result.diagnostics[0]);

    // ALLOW_DIAGNOSTIC_RENDER_TEXT: tool output rendered diagnostic contract

    assert!(output.contains("error[SEM001:unknown_identifier] analysis main.fab"));
    assert!(output.contains("phase: analysis"));
    assert!(output.contains("file: main.fab"));
    assert!(output.contains("span: "));
    assert!(output.contains("source: \u{2068}missing\u{2069}"));
    assert!(!output.contains("help: "));

    let lowered = crate::diagnostics::render_expanded(&result.diagnostics[1]);

    // ALLOW_DIAGNOSTIC_RENDER_TEXT: tool output rendered diagnostic contract
    assert!(lowered.contains("error[SEM008:undefined_name] analysis main.fab"));
    assert!(lowered.contains("source: \u{2068}missing\u{2069}"));
}

#[test]
fn diagnostics_check_uses_locale_pack_for_keyword_lexing() {
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");

    let result =
        check_diagnostics_for_source_with_locale_pack("main.fab", "函数 salve() { }", false, Some(&pack), None);

    assert!(
        result.success,
        "localized functio keyword should reach semantic check, diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error())
    );
}

#[test]
fn diagnostics_check_rejects_undeclared_latin_keyword_in_grammar_position() {
    let pack = zh_locale_pack();
    let result =
        check_diagnostics_for_source_with_locale_pack("main.fab", "functio salve() { }", false, Some(&pack), None);

    // S2-U2 pack-authoritative gate: the zh-Hans pack does not declare the
    // Latin spelling `functio`, so it must NOT claim the function-declaration
    // slot. The word stays an ordinary identifier and the grammar-position use
    // fails normally (SEM001 undefined identifier) — no fallback event, no
    // LOCALE diagnostic.
    assert!(!result.success, "diagnostics: {:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.is_error() && diagnostic.code == Some("SEM001")),
        "undeclared spelling must fail as an ordinary undefined identifier: {:?}",
        result.diagnostics
    );
    assert!(result.diagnostics.iter().all(|diagnostic| {
        !diagnostic
            .code
            .is_some_and(|code| code.starts_with("LOCALE"))
    }));
}

/// Shared fixture: compiles the `add_one` nucleum and returns parsed reflection JSON.
fn gpu_reflection_fixture() -> (String, serde_json::Value) {
    let source = r#"+++
locale = "la"
+++
@ nucleum
@ publica
functio add_one(f32 x) → f32 {
    redde x + 1.0
}
"#;
    let result = compile_cli_source("main.fab", source, crate::codegen::Target::MirWgsl);

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = result.output.expect("wgsl output");
    let json = output_reflection_json(&output)
        .expect("serialize GPU reflection JSON")
        .expect("GPU reflection JSON");
    let reflection: serde_json::Value = serde_json::from_str(&json).expect("reflection JSON");
    (json, reflection)
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_top_level_string_fields() {
    let (json, _reflection) = gpu_reflection_fixture();

    assert!(json.contains(r#""schema_version": 1"#), "{json}");
    assert!(json.contains(r#""target": "wgsl-text""#), "{json}");
    assert!(json.contains(r#""entry_name": "add_one""#), "{json}");
    assert!(json.contains(r#""group": 0"#), "{json}");
    assert!(json.contains(r#""binding": 0"#), "{json}");
    assert!(json.contains(r#""kind": "storage-buffer""#), "{json}");
    assert!(json.contains(r#""role": "input""#), "{json}");
    assert!(json.contains(r#""access": "read""#), "{json}");
    assert!(json.contains(r#""element_layout": "f32""#), "{json}");
    assert!(json.contains(r#""element_byte_width": 4"#), "{json}");
    assert!(json.contains(r#""element_count": 1"#), "{json}");
    assert!(json.contains(r#""buffer_byte_len": 4"#), "{json}");
    assert!(json.contains(r#""source_local": 0"#), "{json}");
    assert!(json.contains(r#""source_name": "x""#), "{json}");
    assert!(json.contains(r#""workgroup""#), "{json}");
    assert!(json.contains(r#""dispatch_size""#), "{json}");
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_kernel_metadata() {
    let (_json, reflection) = gpu_reflection_fixture();
    let kernel = &reflection["kernels"][0];

    assert_eq!(kernel["shader_stage"], serde_json::json!("compute"));
    assert_eq!(kernel["resources"][0]["kind"], serde_json::json!("storage-buffer"));
    assert_eq!(kernel["workgroup_count"]["x"], serde_json::json!(1));
    assert_eq!(kernel["workgroup_count"]["y"], serde_json::json!(1));
    assert_eq!(kernel["workgroup_count"]["z"], serde_json::json!(1));
    assert_eq!(kernel["launch"]["entry_name"], serde_json::json!("add_one"));
    assert_eq!(kernel["launch"]["shader_stage"], serde_json::json!("compute"));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_pipeline_layout() {
    let (_json, reflection) = gpu_reflection_fixture();
    let kernel = &reflection["kernels"][0];

    assert_eq!(
        kernel["launch"]["pipeline_layout"]["bind_group_layout_count"],
        serde_json::json!(1)
    );
    assert_eq!(kernel["launch"]["bind_group_count"], serde_json::json!(1));
    assert_eq!(
        kernel["launch"]["pipeline_layout"]["bind_group_layouts"][0]["bind_group_index"],
        serde_json::json!(0)
    );
    assert_eq!(
        kernel["launch"]["pipeline_layout"]["bind_group_layouts"][0]["group"],
        serde_json::json!(0)
    );
    assert_eq!(
        kernel["launch"]["pipeline_layout"]["bind_group_layouts"][0]["layout_entry_count"],
        serde_json::json!(3)
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_bind_group_resource_metadata_arrays() {
    let (_json, reflection) = gpu_reflection_fixture();
    let bg = &reflection["kernels"][0]["launch"]["bind_groups"][0];

    assert_eq!(bg["bind_group_index"], serde_json::json!(0));
    assert_eq!(bg["group"], serde_json::json!(0));
    assert_eq!(bg["resource_indexes"], serde_json::json!([0, 2, 1]));
    assert_eq!(bg["resource_index_count"], serde_json::json!(3));
    assert_eq!(
        bg["resource_kinds"],
        serde_json::json!(["storage-buffer", "storage-buffer", "runtime-extent"])
    );
    assert_eq!(bg["resource_kind_count"], serde_json::json!(3));
    assert_eq!(bg["resource_roles"], serde_json::json!(["input", "output", "input"]));
    assert_eq!(bg["resource_role_count"], serde_json::json!(3));
    assert_eq!(bg["shader_accesses"], serde_json::json!(["read", "read_write", "read"]));
    assert_eq!(bg["shader_access_count"], serde_json::json!(3));
    assert_eq!(
        bg["buffer_binding_types"],
        serde_json::json!(["read-only-storage", "storage", "read-only-storage"])
    );
    assert_eq!(bg["buffer_binding_type_count"], serde_json::json!(3));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_bind_group_buffer_metadata() {
    let (_json, reflection) = gpu_reflection_fixture();
    let bg = &reflection["kernels"][0]["launch"]["bind_groups"][0];

    assert_eq!(bg["buffer_byte_offsets"], serde_json::json!([0, 0, 0]));
    assert_eq!(bg["buffer_byte_offset_count"], serde_json::json!(3));
    assert_eq!(bg["buffer_byte_lens"], serde_json::json!([4, 4, 4]));
    assert_eq!(bg["buffer_byte_len_count"], serde_json::json!(3));
    assert_eq!(bg["binding_byte_lens"], serde_json::json!([4, 4, 4]));
    assert_eq!(bg["binding_byte_len_count"], serde_json::json!(3));
    assert_eq!(bg["min_binding_sizes"], serde_json::json!([4, 4, 4]));
    assert_eq!(bg["min_binding_size_count"], serde_json::json!(3));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_bind_group_dynamic_offset_flags() {
    let (_json, reflection) = gpu_reflection_fixture();
    let bg = &reflection["kernels"][0]["launch"]["bind_groups"][0];

    assert_eq!(bg["has_dynamic_offsets"], serde_json::json!([false, false, false]));
    assert_eq!(bg["dynamic_offset_flag_count"], serde_json::json!(3));
    assert_eq!(bg["offset_modes"], serde_json::json!(["fixed", "fixed", "fixed"]));
    assert_eq!(bg["offset_mode_entry_count"], serde_json::json!(3));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_bind_group_offset_mode_summaries() {
    let (_json, reflection) = gpu_reflection_fixture();
    let bg = &reflection["kernels"][0]["launch"]["bind_groups"][0];

    assert_eq!(bg["requires_dynamic_offsets"], serde_json::json!(false));
    assert_eq!(bg["has_non_dynamic_offsets"], serde_json::json!(true));
    assert_eq!(bg["has_mixed_offset_modes"], serde_json::json!(false));
    assert_eq!(bg["offset_mode_count"], serde_json::json!(1));
    assert_eq!(bg["has_only_non_dynamic_offsets"], serde_json::json!(true));
    assert_eq!(bg["has_only_dynamic_offsets"], serde_json::json!(false));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_bind_group_non_dynamic_offset_details() {
    let (_json, reflection) = gpu_reflection_fixture();
    let bg = &reflection["kernels"][0]["launch"]["bind_groups"][0];

    assert_eq!(bg["non_dynamic_offset_count"], serde_json::json!(3));
    assert_eq!(bg["non_dynamic_offset_resource_indexes"], serde_json::json!([0, 2, 1]));
    assert_eq!(bg["non_dynamic_offset_resource_index_count"], serde_json::json!(3));
    assert_eq!(bg["non_dynamic_offset_binding_indexes"], serde_json::json!([0, 1, 2]));
    assert_eq!(bg["non_dynamic_offset_binding_index_count"], serde_json::json!(3));
    assert_eq!(bg["non_dynamic_offset_layout_entry_indexes"], serde_json::json!([0, 1, 2]));
    assert_eq!(bg["non_dynamic_offset_layout_entry_index_count"], serde_json::json!(3));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_bind_group_dynamic_offset_details() {
    let (_json, reflection) = gpu_reflection_fixture();
    let bg = &reflection["kernels"][0]["launch"]["bind_groups"][0];

    assert_eq!(bg["dynamic_offset_count"], serde_json::json!(0));
    assert_eq!(bg["dynamic_offset_resource_indexes"], serde_json::json!([]));
    assert_eq!(bg["dynamic_offset_resource_index_count"], serde_json::json!(0));
    assert_eq!(bg["dynamic_offset_binding_indexes"], serde_json::json!([]));
    assert_eq!(bg["dynamic_offset_binding_index_count"], serde_json::json!(0));
    assert_eq!(bg["dynamic_offset_layout_entry_indexes"], serde_json::json!([]));
    assert_eq!(bg["dynamic_offset_layout_entry_index_count"], serde_json::json!(0));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_bind_group_indexes_and_sources() {
    let (_json, reflection) = gpu_reflection_fixture();
    let bg = &reflection["kernels"][0]["launch"]["bind_groups"][0];

    assert_eq!(bg["binding_indexes"], serde_json::json!([0, 1, 2]));
    assert_eq!(bg["binding_index_count"], serde_json::json!(3));
    assert_eq!(bg["layout_entry_indexes"], serde_json::json!([0, 1, 2]));
    assert_eq!(bg["layout_entry_index_count"], serde_json::json!(3));
    assert_eq!(bg["source_locals"], serde_json::json!([0]));
    assert_eq!(bg["source_local_count"], serde_json::json!(1));
    assert_eq!(bg["source_names"], serde_json::json!(["x", "runtime_extent"]));
    assert_eq!(bg["source_name_count"], serde_json::json!(2));
    assert_eq!(bg["binding_count"], serde_json::json!(3));
    assert_eq!(bg["layout_entry_count"], serde_json::json!(3));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_layout_entries_input() {
    let (_json, reflection) = gpu_reflection_fixture();
    let entries = &reflection["kernels"][0]["launch"]["bind_groups"][0]["layout_entries"];

    assert_eq!(entries[0]["layout_entry_index"], serde_json::json!(0));
    assert_eq!(entries[0]["resource_index"], serde_json::json!(0));
    assert_eq!(entries[0]["binding"], serde_json::json!(0));
    assert_eq!(entries[0]["shader_visibility"], serde_json::json!("compute"));
    assert_eq!(entries[0]["buffer_binding_type"], serde_json::json!("read-only-storage"));
    assert_eq!(entries[0]["min_binding_size"], serde_json::json!(4));
    assert_eq!(entries[0]["has_dynamic_offset"], serde_json::json!(false));
    assert_eq!(entries[0]["source_local"], serde_json::json!(0));
    assert_eq!(entries[0]["source_name"], serde_json::json!("x"));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_layout_entries_output() {
    let (_json, reflection) = gpu_reflection_fixture();
    let entries = &reflection["kernels"][0]["launch"]["bind_groups"][0]["layout_entries"];

    assert_eq!(entries[1]["layout_entry_index"], serde_json::json!(1));
    assert_eq!(entries[1]["resource_index"], serde_json::json!(2));
    assert_eq!(entries[1]["binding"], serde_json::json!(1));
    assert_eq!(entries[1]["shader_visibility"], serde_json::json!("compute"));
    assert_eq!(entries[1]["buffer_binding_type"], serde_json::json!("storage"));
    assert_eq!(entries[1]["min_binding_size"], serde_json::json!(4));
    assert_eq!(entries[1]["has_dynamic_offset"], serde_json::json!(false));
    assert_eq!(entries[1]["source_local"], serde_json::Value::Null);
    assert_eq!(entries[1]["source_name"], serde_json::Value::Null);
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_bindings_input() {
    let (_json, reflection) = gpu_reflection_fixture();
    let kernel = &reflection["kernels"][0];
    let b = &kernel["launch"]["bind_groups"][0]["bindings"][0];

    assert_eq!(b["resource_index"], serde_json::json!(0));
    assert_eq!(b["binding_index"], serde_json::json!(0));
    assert_eq!(b["binding"], serde_json::json!(0));
    assert_eq!(b["kind"], serde_json::json!("storage-buffer"));
    assert_eq!(b["role"], serde_json::json!("input"));
    assert_eq!(b["access"], serde_json::json!("read"));
    assert_eq!(b["shader_access"], serde_json::json!("read"));
    assert_eq!(b["shader_visibility"], serde_json::json!("compute"));
    assert_eq!(b["buffer_binding_type"], serde_json::json!("read-only-storage"));
    assert_eq!(b["element_type"], kernel["resources"][0]["element_type"]);
    assert_eq!(b["element_layout"], serde_json::json!("f32"));
    assert_eq!(b["element_byte_width"], serde_json::json!(4));
    assert_eq!(b["element_count"], serde_json::json!(1));
    assert_eq!(b["buffer_byte_len"], serde_json::json!(4));
    assert_eq!(b["buffer_byte_offset"], serde_json::json!(0));
    assert_eq!(b["binding_byte_len"], serde_json::json!(4));
    assert_eq!(b["min_binding_size"], serde_json::json!(4));
    assert_eq!(b["has_dynamic_offset"], serde_json::json!(false));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_bindings_output() {
    let (_json, reflection) = gpu_reflection_fixture();
    let b = &reflection["kernels"][0]["launch"]["bind_groups"][0]["bindings"][1];

    assert_eq!(b["resource_index"], serde_json::json!(2));
    assert_eq!(b["binding_index"], serde_json::json!(1));
    assert_eq!(b["shader_access"], serde_json::json!("read_write"));
    assert_eq!(b["shader_visibility"], serde_json::json!("compute"));
    assert_eq!(b["buffer_binding_type"], serde_json::json!("storage"));
    assert_eq!(b["buffer_byte_offset"], serde_json::json!(0));
    assert_eq!(b["binding_byte_len"], serde_json::json!(4));
    assert_eq!(b["min_binding_size"], serde_json::json!(4));
    assert_eq!(b["has_dynamic_offset"], serde_json::json!(false));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_adapter_top_level_keys() {
    let (_json, reflection) = gpu_reflection_fixture();
    let webgpu = &reflection["kernels"][0]["launch"]["webgpu_adapter"];

    assert_json_object_keys(
        webgpu,
        &[
            "bind_group_descriptor_count",
            "bind_group_descriptor_index_count",
            "bind_group_descriptor_indexes",
            "bind_group_descriptors",
            "bind_group_layout_descriptor_count",
            "bind_group_layout_descriptor_index_count",
            "bind_group_layout_descriptor_indexes",
            "bind_group_layout_descriptors",
            "dispatch_workgroup_dimension_count",
            "dispatch_workgroups",
            "pipeline_layout_descriptor",
            "vertex_buffer_layout_descriptor_count",
            "vertex_buffer_layout_descriptor_index_count",
            "vertex_buffer_layout_descriptor_indexes",
            "vertex_buffer_layout_descriptors",
        ],
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_pipeline_layout_descriptor() {
    let (_json, reflection) = gpu_reflection_fixture();
    let pd = &reflection["kernels"][0]["launch"]["webgpu_adapter"]["pipeline_layout_descriptor"];

    assert_json_object_keys(
        pd,
        &[
            "bind_group_layout_count",
            "bind_group_layout_index_count",
            "bind_group_layout_indexes",
        ],
    );
    assert_eq!(pd["bind_group_layout_count"], serde_json::json!(1));
    assert_eq!(pd["bind_group_layout_indexes"], serde_json::json!([0]));
    assert_eq!(pd["bind_group_layout_index_count"], serde_json::json!(1));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_descriptor_counts() {
    let (_json, reflection) = gpu_reflection_fixture();
    let webgpu = &reflection["kernels"][0]["launch"]["webgpu_adapter"];

    assert_eq!(webgpu["bind_group_layout_descriptor_count"], serde_json::json!(1));
    assert_eq!(webgpu["bind_group_layout_descriptor_indexes"], serde_json::json!([0]));
    assert_eq!(webgpu["bind_group_layout_descriptor_index_count"], serde_json::json!(1));
    assert_eq!(webgpu["bind_group_descriptor_count"], serde_json::json!(1));
    assert_eq!(webgpu["bind_group_descriptor_indexes"], serde_json::json!([0]));
    assert_eq!(webgpu["bind_group_descriptor_index_count"], serde_json::json!(1));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_bind_group_layout_descriptor() {
    let (_json, reflection) = gpu_reflection_fixture();
    let ld = &reflection["kernels"][0]["launch"]["webgpu_adapter"]["bind_group_layout_descriptors"][0];

    assert_json_object_keys(
        ld,
        &[
            "bind_group_index",
            "entries",
            "entry_count",
            "group",
            "layout_entry_index_count",
            "layout_entry_indexes",
        ],
    );
    assert_eq!(ld["bind_group_index"], serde_json::json!(0));
    assert_eq!(ld["group"], serde_json::json!(0));
    assert_eq!(ld["layout_entry_indexes"], serde_json::json!([0, 1, 2]));
    assert_eq!(ld["layout_entry_index_count"], serde_json::json!(3));
    assert_eq!(ld["entry_count"], serde_json::json!(3));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_layout_entry_input() {
    let (_json, reflection) = gpu_reflection_fixture();
    let le = &reflection["kernels"][0]["launch"]["webgpu_adapter"]["bind_group_layout_descriptors"][0]["entries"][0];

    assert_json_object_keys(
        le,
        &[
            "binding",
            "binding_byte_len",
            "binding_index",
            "buffer_byte_len",
            "buffer_byte_offset",
            "buffer_type",
            "has_dynamic_offset",
            "layout_entry_index",
            "min_binding_size",
            "resource_index",
            "source_local",
            "source_name",
            "visibility",
        ],
    );
    assert_eq!(le["binding"], serde_json::json!(0));
    assert_eq!(le["binding_index"], serde_json::json!(0));
    assert_eq!(le["resource_index"], serde_json::json!(0));
    assert_eq!(le["layout_entry_index"], serde_json::json!(0));
    assert_eq!(le["buffer_byte_len"], serde_json::json!(4));
    assert_eq!(le["buffer_byte_offset"], serde_json::json!(0));
    assert_eq!(le["binding_byte_len"], serde_json::json!(4));
    assert_eq!(le["visibility"], serde_json::json!("compute"));
    assert_eq!(le["buffer_type"], serde_json::json!("read-only-storage"));
    assert_eq!(le["min_binding_size"], serde_json::json!(4));
    assert_eq!(le["has_dynamic_offset"], serde_json::json!(false));
    assert_eq!(le["source_local"], serde_json::json!(0));
    assert_eq!(le["source_name"], serde_json::json!("x"));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_layout_entry_output() {
    let (_json, reflection) = gpu_reflection_fixture();
    let le = &reflection["kernels"][0]["launch"]["webgpu_adapter"]["bind_group_layout_descriptors"][0]["entries"][1];

    assert_json_object_keys(
        le,
        &[
            "binding",
            "binding_byte_len",
            "binding_index",
            "buffer_byte_len",
            "buffer_byte_offset",
            "buffer_type",
            "has_dynamic_offset",
            "layout_entry_index",
            "min_binding_size",
            "resource_index",
            "source_local",
            "visibility",
        ],
    );
    assert_eq!(le["binding"], serde_json::json!(1));
    assert_eq!(le["binding_index"], serde_json::json!(1));
    assert_eq!(le["buffer_byte_len"], serde_json::json!(4));
    assert_eq!(le["buffer_byte_offset"], serde_json::json!(0));
    assert_eq!(le["binding_byte_len"], serde_json::json!(4));
    assert_eq!(le["visibility"], serde_json::json!("compute"));
    assert_eq!(le["buffer_type"], serde_json::json!("storage"));
    assert_eq!(le["min_binding_size"], serde_json::json!(4));
    assert_eq!(le["has_dynamic_offset"], serde_json::json!(false));
    assert_eq!(le["resource_index"], serde_json::json!(2));
    assert_eq!(le["layout_entry_index"], serde_json::json!(1));
    assert_eq!(le["source_local"], serde_json::Value::Null);
    assert_eq!(le["source_name"], serde_json::Value::Null);
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_bind_group_descriptor() {
    let (_json, reflection) = gpu_reflection_fixture();
    let bgd = &reflection["kernels"][0]["launch"]["webgpu_adapter"]["bind_group_descriptors"][0];

    assert_json_object_keys(
        bgd,
        &[
            "bind_group_index",
            "entries",
            "entry_count",
            "entry_index_count",
            "entry_indexes",
            "group",
        ],
    );
    assert_eq!(bgd["bind_group_index"], serde_json::json!(0));
    assert_eq!(bgd["group"], serde_json::json!(0));
    assert_eq!(bgd["entry_indexes"], serde_json::json!([0, 1, 2]));
    assert_eq!(bgd["entry_index_count"], serde_json::json!(3));
    assert_eq!(bgd["entry_count"], serde_json::json!(3));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_bind_group_entry_input() {
    let (_json, reflection) = gpu_reflection_fixture();
    let kernel = &reflection["kernels"][0];
    let entry = &kernel["launch"]["webgpu_adapter"]["bind_group_descriptors"][0]["entries"][0];

    assert_json_object_keys(
        entry,
        &[
            "access",
            "binding",
            "binding_byte_len",
            "binding_index",
            "buffer_byte_len",
            "buffer_byte_offset",
            "buffer_type",
            "element_byte_width",
            "element_count",
            "element_layout",
            "element_type",
            "has_dynamic_offset",
            "kind",
            "min_binding_size",
            "resource_index",
            "role",
            "shader_access",
            "shader_visibility",
            "source_local",
            "source_name",
        ],
    );
    assert_eq!(entry["binding"], serde_json::json!(0));
    assert_eq!(entry["kind"], serde_json::json!("storage-buffer"));
    assert_eq!(entry["role"], serde_json::json!("input"));
    assert_eq!(entry["access"], serde_json::json!("read"));
    assert_eq!(entry["shader_access"], serde_json::json!("read"));
    assert_eq!(entry["shader_visibility"], serde_json::json!("compute"));
    assert_eq!(entry["element_type"], kernel["resources"][0]["element_type"]);
    assert_eq!(entry["element_layout"], serde_json::json!("f32"));
    assert_eq!(entry["element_byte_width"], serde_json::json!(4));
    assert_eq!(entry["element_count"], serde_json::json!(1));
    assert_eq!(entry["resource_index"], serde_json::json!(0));
    assert_eq!(entry["binding_index"], serde_json::json!(0));
    assert_eq!(entry["buffer_type"], serde_json::json!("read-only-storage"));
    assert_eq!(entry["buffer_byte_len"], serde_json::json!(4));
    assert_eq!(entry["buffer_byte_offset"], serde_json::json!(0));
    assert_eq!(entry["binding_byte_len"], serde_json::json!(4));
    assert_eq!(entry["min_binding_size"], serde_json::json!(4));
    assert_eq!(entry["has_dynamic_offset"], serde_json::json!(false));
    assert_eq!(entry["source_local"], serde_json::json!(0));
    assert_eq!(entry["source_name"], serde_json::json!("x"));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_bind_group_entry_output() {
    let (_json, reflection) = gpu_reflection_fixture();
    let kernel = &reflection["kernels"][0];
    let entry = &kernel["launch"]["webgpu_adapter"]["bind_group_descriptors"][0]["entries"][1];

    assert_json_object_keys(
        entry,
        &[
            "access",
            "binding",
            "binding_byte_len",
            "binding_index",
            "buffer_byte_len",
            "buffer_byte_offset",
            "buffer_type",
            "element_byte_width",
            "element_count",
            "element_layout",
            "element_type",
            "has_dynamic_offset",
            "kind",
            "min_binding_size",
            "resource_index",
            "role",
            "shader_access",
            "shader_visibility",
            "source_local",
        ],
    );
    assert_eq!(entry["binding"], serde_json::json!(1));
    assert_eq!(entry["kind"], serde_json::json!("storage-buffer"));
    assert_eq!(entry["role"], serde_json::json!("output"));
    assert_eq!(entry["access"], serde_json::json!("write"));
    assert_eq!(entry["shader_access"], serde_json::json!("read_write"));
    assert_eq!(entry["shader_visibility"], serde_json::json!("compute"));
    assert_eq!(entry["element_type"], kernel["resources"][2]["element_type"]);
    assert_eq!(entry["element_layout"], serde_json::json!("f32"));
    assert_eq!(entry["element_byte_width"], serde_json::json!(4));
    assert_eq!(entry["element_count"], serde_json::json!(1));
    assert_eq!(entry["resource_index"], serde_json::json!(2));
    assert_eq!(entry["binding_index"], serde_json::json!(1));
    assert_eq!(entry["buffer_type"], serde_json::json!("storage"));
    assert_eq!(entry["buffer_byte_len"], serde_json::json!(4));
    assert_eq!(entry["buffer_byte_offset"], serde_json::json!(0));
    assert_eq!(entry["binding_byte_len"], serde_json::json!(4));
    assert_eq!(entry["min_binding_size"], serde_json::json!(4));
    assert_eq!(entry["has_dynamic_offset"], serde_json::json!(false));
    assert_eq!(entry["source_local"], serde_json::Value::Null);
    assert_eq!(entry["source_name"], serde_json::Value::Null);
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_webgpu_dispatch_and_builtins() {
    let (_json, reflection) = gpu_reflection_fixture();
    let kernel = &reflection["kernels"][0];
    let webgpu = &kernel["launch"]["webgpu_adapter"];

    assert_json_object_keys(&webgpu["dispatch_workgroups"], &["x", "y", "z"]);
    assert_eq!(webgpu["dispatch_workgroup_dimension_count"], serde_json::json!(3));
    assert_eq!(webgpu["dispatch_workgroups"]["x"], serde_json::json!(1));
    assert_eq!(webgpu["dispatch_workgroups"]["y"], serde_json::json!(1));
    assert_eq!(webgpu["dispatch_workgroups"]["z"], serde_json::json!(1));
    assert_eq!(kernel["launch"]["workgroup_count"]["x"], serde_json::json!(1));
    assert_eq!(kernel["launch"]["required_builtins"], serde_json::json!([]));
    assert_eq!(kernel["launch"]["builtin_inputs"], serde_json::json!([]));
    assert_eq!(kernel["launch"]["called_builtins"], serde_json::json!([]));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn gpu_builtin_emit_reflection_json_serializes_source_names_for_builtin_inputs() {
    let source = r#"+++
locale = "la"
+++
@ nucleum
@ publica
functio copy(tf32[4] input, varia tf32[4] output, u32 id) → vacuum {
    output[id] ← input[id]
}
"#;
    let result = compile_cli_source("main.fab", source, crate::codegen::Target::MirWgsl);

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = result.output.expect("wgsl output");
    let json = output_reflection_json(&output)
        .expect("serialize GPU reflection JSON")
        .expect("GPU reflection JSON");

    assert!(json.contains(r#""builtin_inputs""#), "{json}");
    assert!(json.contains(r#""builtin": "global_invocation_x""#), "{json}");
    assert!(json.contains(r#""axis": "x""#), "{json}");
    assert!(json.contains(r#""source_local": 2"#), "{json}");
    assert!(json.contains(r#""source_name": "id""#), "{json}");
    assert!(json.contains(r#""layout": "u32""#), "{json}");
    assert!(json.contains(r#""dispatch_size""#), "{json}");
    let reflection: serde_json::Value = serde_json::from_str(&json).expect("reflection JSON");
    let kernel = &reflection["kernels"][0];
    assert_eq!(kernel["resources"][0]["source_name"], serde_json::json!("input"));
    assert_eq!(kernel["resources"][1]["source_name"], serde_json::json!("output"));
    assert_eq!(kernel["builtin_inputs"][0]["axis"], serde_json::json!("x"));
    assert_eq!(kernel["builtin_inputs"][0]["source_name"], serde_json::json!("id"));
    assert_eq!(kernel["workgroup_count"]["x"], serde_json::json!(4));
    assert_eq!(kernel["workgroup_count"]["y"], serde_json::json!(1));
    assert_eq!(kernel["workgroup_count"]["z"], serde_json::json!(1));
}

#[test]
fn gpu_builtin_named_parameters_are_rejected_as_raw_lane_ids() {
    // A trailing parameter named after a thread-id builtin is a raw lane id
    // under another spelling (gpu-reset rule 12.5). The compiler still binds
    // its own builtin inputs, but only for legal parameter names.
    let source = r#"+++
locale = "la"
+++
@ nucleum
@ publica
functio copy(tensor<u32, [4]> input, tensor<u32, [4]> output, u32 global_invocation_x, u32 workgroup_id_y) → vacuum {
    output[global_invocation_x] ← input[global_invocation_x]
}
"#;
    let result = compile_cli_source("main.fab", source, crate::codegen::Target::MirWgsl);

    assert!(!result.success(), "{:?}", result.diagnostics);
    for parameter in ["global_invocation_x", "workgroup_id_y"] {
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.issue() == Some("nucleum_raw_lane_id_param")
                    && diagnostic
                        .args
                        .contains(&crate::diagnostics::DiagnosticArg::new("parameter", parameter))
                    && diagnostic
                        .args
                        .contains(&crate::diagnostics::DiagnosticArg::new("builtin", parameter))
            }),
            "{parameter}: {:?}",
            result.diagnostics
        );
    }
}

#[cfg(feature = "mir-metal")]
#[test]
fn emit_reflection_json_serializes_metal_target_contract() {
    let reflection = crate::mir::MirGpuReflection { kernels: Vec::new(), pipeline: None };
    let output = crate::Output::MetalText(crate::MetalTextOutput { code: String::new(), reflection: Some(reflection) });

    let json = output_reflection_json(&output)
        .expect("serialize GPU reflection JSON")
        .expect("GPU reflection JSON");
    let reflection: serde_json::Value = serde_json::from_str(&json).expect("reflection JSON");

    assert_eq!(reflection["schema_version"], serde_json::json!(1));
    assert_eq!(reflection["target"], serde_json::json!("metal-text"));
    assert_eq!(
        reflection["kernels"]
            .as_array()
            .expect("kernel array")
            .len(),
        0
    );
}

fn naga_available() -> bool {
    std::process::Command::new("naga")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn validate_wgsl_with_naga(source: &str, fixture_name: &str) {
    if !naga_available() {
        eprintln!("naga not found on PATH; skipping {fixture_name} WGSL validation");
        return;
    }
    let dir = crate::test_support::test_temp_dir(&format!("tool-wgsl-naga-{fixture_name}"));
    let path = dir.path().join(format!("{fixture_name}.wgsl"));
    std::fs::write(&path, source).expect("write temp wgsl");
    let validate = std::process::Command::new("naga")
        .arg(&path)
        .output()
        .expect("run naga; install with `cargo install naga-cli --version 30.0.0`");
    assert!(
        validate.status.success(),
        "naga rejected {fixture_name} WGSL\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&validate.stdout),
        String::from_utf8_lossy(&validate.stderr)
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_serializes_vertex_input_layout_fact() {
    let vertex = crate::mir::MirKernelReflection::vertex_entry(
        crate::mir::MirFunctionId(7),
        "plane_vertex",
        vec![crate::mir::MirVertexInputReflection {
            source_name: Some("position".to_owned()),
            location: 0,
            format: crate::mir::MirVertexInputFormat::Float32x3,
            step_mode: crate::mir::MirInputStepMode::Vertex,
            offset_bytes: 0,
            stride_bytes: 12,
            role: radix_mir::ShaderStageIoRole::Other,
        }],
        vec![],
    )
    .expect("vertex reflection");
    let reflection = crate::mir::MirGpuReflection { kernels: vec![vertex], pipeline: None };
    let output = crate::Output::WgslText(crate::WgslTextOutput { code: String::new(), reflection: Some(reflection) });

    let json = output_reflection_json(&output)
        .expect("serialize GPU reflection JSON")
        .expect("GPU reflection JSON");
    let reflection: serde_json::Value = serde_json::from_str(&json).expect("reflection JSON");
    let kernel = &reflection["kernels"][0];
    let input = &kernel["vertex_inputs"][0];

    assert_eq!(kernel["function"], serde_json::json!(7));
    assert_eq!(kernel["provenance"]["kind"], serde_json::json!("mir-function"));
    assert_eq!(kernel["provenance"]["function"], serde_json::json!(7));
    assert_eq!(kernel["shader_stage"], serde_json::json!("vertex"));
    assert_eq!(kernel["vertex_input_count"], serde_json::json!(1));
    assert_eq!(input["source_name"], serde_json::json!("position"));
    assert_eq!(input["location"], serde_json::json!(0));
    assert_eq!(input["format"], serde_json::json!("float32x3"));
    assert_eq!(input["step_mode"], serde_json::json!("vertex"));
    assert_eq!(input["offset_bytes"], serde_json::json!(0));
    assert_eq!(input["stride_bytes"], serde_json::json!(12));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn shader_contract_source_vertex_entry_emits_wgsl_contract() {
    let result = compile_shader_contract_source_fixture();
    assert!(result.success(), "{:?}", result.diagnostics);
    let (wgsl, reflection) = wgsl_output_reflection(result);
    let vertex = &reflection.kernels[0];

    assert_eq!(vertex.shader_stage, crate::mir::MirKernelShaderStage::Vertex);
    assert_eq!(vertex.entry_name, "shader_plane_vertex");
    assert_eq!(vertex.vertex_inputs.len(), 3);
    assert!(wgsl.contains("@vertex"));
    assert!(wgsl.contains("fn shader_plane_vertex(input: ShaderPlaneVertexInput) -> ShaderPlaneVertexOutput"));
    assert!(wgsl.contains("@location(0) position: vec3<f32>"));
    assert!(wgsl.contains("@location(1) normal: vec3<f32>"));
    assert!(wgsl.contains("@location(2) uv: vec2<f32>"));
    assert!(wgsl.contains("@builtin(position) position: vec4<f32>"));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn shader_contract_source_vertex_entry_validates_with_naga() {
    let result = compile_shader_contract_source_fixture();
    assert!(result.success(), "{:?}", result.diagnostics);
    let (wgsl, _) = wgsl_output_reflection(result);

    if !naga_available() {
        eprintln!("naga CLI not found on PATH; skipping shader-contract source vertex-entry naga gate");
        return;
    }
    validate_wgsl_with_naga(&wgsl, "shader-contract-source-vertex-entry");
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn shader_contract_source_vertex_entry_emits_validated_wgsl_through_cli_path() {
    let result = compile_shader_contract_source_fixture();
    assert!(result.success(), "{:?}", result.diagnostics);
    let output = result.output.expect("wgsl output");
    let crate::Output::WgslText(wgsl_output) = &output else {
        panic!("expected WGSL output");
    };
    let reflection = wgsl_output.reflection.as_ref().expect("WGSL reflection");
    let vertex = &reflection.kernels[0];

    assert_eq!(vertex.shader_stage, crate::mir::MirKernelShaderStage::Vertex);
    assert_eq!(vertex.entry_name, "shader_plane_vertex");
    assert_eq!(vertex.vertex_inputs.len(), 3);
    let source_def = vertex
        .source_function_provenance()
        .expect("source vertex reflection must carry source definition provenance");
    assert!(wgsl_output.code.contains("@vertex"));
    assert!(wgsl_output.code.contains("fn shader_plane_vertex"));
    let json = output_reflection_json(&output)
        .expect("serialize source vertex reflection")
        .expect("source vertex reflection JSON");
    let reflection_json: serde_json::Value = serde_json::from_str(&json).expect("source vertex reflection JSON");
    let kernel_json = &reflection_json["kernels"][0];
    assert_eq!(kernel_json.get("function"), None);
    assert_eq!(kernel_json["provenance"]["kind"], serde_json::json!("source-function"));
    assert_eq!(kernel_json["provenance"]["source_def"], serde_json::json!(source_def.0));
    if !naga_available() {
        eprintln!("naga CLI not found on PATH; skipping shader-contract CLI-path vertex-entry naga gate");
        return;
    }
    validate_wgsl_with_naga(&wgsl_output.code, "shader-contract-cli-path-vertex-entry");
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_vertex_names_ignore_assertion_message_text() {
    let source = include_str!("../fixtures/shader-contract-source-facts.fab")
        .replace(
            "position layout fixes location 0, Float32x3, zero offset, 12-byte stride, and vertex step mode",
            "normal prose must not drive vertex ABI identity",
        )
        .replace(
            "normal layout fixes location 1, Float32x3, zero offset, 12-byte stride, and vertex step mode",
            "uv prose must not drive vertex ABI identity",
        )
        .replace(
            "uv layout fixes location 2, Float32x2, zero offset, 8-byte stride, and vertex step mode",
            "position prose must not drive vertex ABI identity",
        );
    let (_td, file) = temp_fab("vertex-source-prose-ignored", &source);
    let path = PathBuf::from(&file);
    let result = compile_cli_path(&path, false, crate::codegen::Target::MirWgsl);
    let _ = std::fs::remove_file(&path);

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = result.output.expect("wgsl output");
    let crate::Output::WgslText(wgsl_output) = &output else {
        panic!("expected WGSL output");
    };
    let reflection = wgsl_output.reflection.as_ref().expect("WGSL reflection");
    let vertex = &reflection.kernels[0];

    assert_eq!(vertex.vertex_inputs.len(), 3);
    assert_eq!(vertex.vertex_inputs[0].source_name.as_deref(), Some("position"));
    assert_eq!(vertex.vertex_inputs[1].source_name.as_deref(), Some("normal"));
    assert_eq!(vertex.vertex_inputs[2].source_name.as_deref(), Some("uv"));
    assert!(
        wgsl_output
            .code
            .contains("@location(0) position: vec3<f32>")
    );
    assert!(wgsl_output.code.contains("@location(1) normal: vec3<f32>"));
    assert!(wgsl_output.code.contains("@location(2) uv: vec2<f32>"));
}

fn assert_shader_contract_vertex_source_variant_rejects(source: &str, issue: &str) {
    let (_td, file) = temp_fab("vertex-source-invalid", source);
    let path = PathBuf::from(&file);
    let result = compile_cli_path(&path, false, crate::codegen::Target::MirWgsl);
    let _ = std::fs::remove_file(&path);

    assert!(!result.success(), "invalid source vertex entry must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some(issue)),
        "expected issue {issue}, got {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_vertex_entry_rejects_annotation_payload() {
    let source = include_str!("../fixtures/shader-contract-source-facts.fab")
        .replace("@ vertex", "@ vertex { mode = \"main\" }");

    assert_shader_contract_vertex_source_variant_rejects(&source, "mir_wgsl_vertex_source_annotation_payload");
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_vertex_entry_rejects_parameters() {
    let source = include_str!("../fixtures/shader-contract-source-facts.fab").replace(
        "functio shader_plane_vertex() → vacuum",
        "functio shader_plane_vertex(numerus x) → vacuum",
    );

    assert_shader_contract_vertex_source_variant_rejects(&source, "mir_wgsl_vertex_source_params");
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_vertex_entry_rejects_non_vacuum_return() {
    // The source body now lowers far enough for WGSL entry validation to report
    // the non-vacuum return directly.
    let source = include_str!("../fixtures/shader-contract-source-facts.fab").replace(
        "functio shader_plane_vertex() → vacuum {\n}",
        "functio shader_plane_vertex() → numerus {\n    redde 1\n}",
    );

    assert_shader_contract_vertex_source_variant_rejects(&source, "mir_wgsl_vertex_source_return");
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_vertex_entry_rejects_error_type() {
    let source = include_str!("../fixtures/shader-contract-source-facts.fab").replace(
        "functio shader_plane_vertex() → vacuum",
        "functio shader_plane_vertex() → vacuum ⇥ textus",
    );

    assert_shader_contract_vertex_source_variant_rejects(&source, "mir_wgsl_vertex_source_error_type");
}

#[test]
fn wgsl_text_source_vertex_entry_accepts_non_empty_body() {
    // Non-empty vertex body is no longer rejected (U4 gate removal).
    // The body uses `tacet` (a no-op statement that passes semantic analysis)
    // to prove the rejection gate is gone. Full MIR-body splice is covered
    // by the shader-contract integration tests when implicit locals are available.
    let source = include_str!("../fixtures/shader-contract-source-facts.fab").replace(
        "functio shader_plane_vertex() → vacuum {\n}",
        "functio shader_plane_vertex() → vacuum {\n    tacet\n}",
    );

    let (_td, file) = temp_fab("vertex-source-nonempty-body", &source);
    let path = PathBuf::from(&file);
    let result = compile_cli_path(&path, false, crate::codegen::Target::MirWgsl);
    let _ = std::fs::remove_file(&path);

    // The vertex body gate is removed — the compilation should proceed
    // (it may still fail on other grounds but NOT on mir_wgsl_vertex_source_body).
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("mir_wgsl_vertex_source_body")),
        "mir_wgsl_vertex_source_body rejection gate must be removed: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_vertex_entry_rejects_missing_layout_facts() {
    let source = r#"+++
locale = "la"
+++
@ vertex
functio lonely_vertex() → vacuum {
}
"#;
    let result = compile_cli_source("missing-layout.fab", source, crate::codegen::Target::MirWgsl);

    assert!(!result.success(), "source vertex without layout facts must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("mir_wgsl_vertex_source_layout_missing")),
        "{:?}",
        result.diagnostics
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_vertex_entry_rejects_layout_without_position() {
    let source = include_str!("../fixtures/shader-contract-source-facts.fab").replace(
        r#"    adfirma vertex_layout_matches(0, 0, "position", 0, 3, 0, 12, 1, 1) mori "position layout fixes location 0, Float32x3, zero offset, 12-byte stride, and vertex step mode"
"#,
        "",
    );
    let (_td, file) = temp_fab("vertex-missing-position", &source);
    let path = PathBuf::from(&file);
    let result = compile_cli_path(&path, false, crate::codegen::Target::MirWgsl);
    let _ = std::fs::remove_file(&path);

    assert!(!result.success(), "source vertex without position layout must fail closed");
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.issue() == Some("mir_wgsl_text_unsupported")
                && diagnostic
                    .args
                    .contains(&crate::diagnostics::DiagnosticArg::new(
                        "shape",
                        "wgsl vertex entry requires a float32x3 input with the position stage-I/O role",
                    ))
        }),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn wgsl_text_source_vertex_entry_rejects_duplicate_layout_location() {
    // U2's identity-resolved ingestion rejects a duplicate vertex-input
    // location at contract-collection time (fail-closed, never merged).
    let source = include_str!("../fixtures/shader-contract-source-facts.fab").replace(
        "vertex_layout_matches(0, 1, \"normal\", 1, 3, 0, 12, 1, 4)",
        "vertex_layout_matches(0, 1, \"normal\", 0, 3, 0, 12, 1, 4)",
    );
    let (_td, file) = temp_fab("vertex-duplicate-layout-location", &source);
    let path = PathBuf::from(&file);
    let result = compile_cli_path(&path, false, crate::codegen::Target::MirWgsl);
    let _ = std::fs::remove_file(&path);

    assert!(!result.success(), "duplicate source vertex location must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("shader_contract_fact_conflict")),
        "{:?}",
        result.diagnostics
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn shader_contract_vertex_layout_facts_extracted_correctly() {
    // U2's identity-resolved ingestion extracts the three vertex-layout
    // facts from the triga-free fixture's contract calls. The canonical
    // reflection fields prove the full fact surface survived ingestion.
    let result = compile_shader_contract_source_fixture();
    assert!(result.success(), "{:?}", result.diagnostics);
    let (_, reflection) = wgsl_output_reflection(result);
    let vertex = &reflection.kernels[0];
    let inputs = &vertex.vertex_inputs;
    assert_eq!(inputs.len(), 3);

    assert_eq!(inputs[0].source_name.as_deref(), Some("position"));
    assert_eq!(inputs[0].location, 0);
    assert_eq!(inputs[0].format, crate::mir::MirVertexInputFormat::Float32x3);
    assert_eq!(inputs[0].offset_bytes, 0);
    assert_eq!(inputs[0].stride_bytes, 12);
    assert_eq!(inputs[0].step_mode, crate::mir::MirInputStepMode::Vertex);

    assert_eq!(inputs[1].source_name.as_deref(), Some("normal"));
    assert_eq!(inputs[1].location, 1);
    assert_eq!(inputs[1].format, crate::mir::MirVertexInputFormat::Float32x3);
    assert_eq!(inputs[1].offset_bytes, 0);
    assert_eq!(inputs[1].stride_bytes, 12);
    assert_eq!(inputs[1].step_mode, crate::mir::MirInputStepMode::Vertex);

    assert_eq!(inputs[2].source_name.as_deref(), Some("uv"));
    assert_eq!(inputs[2].location, 2);
    assert_eq!(inputs[2].format, crate::mir::MirVertexInputFormat::Float32x2);
    assert_eq!(inputs[2].offset_bytes, 0);
    assert_eq!(inputs[2].stride_bytes, 8);
    assert_eq!(inputs[2].step_mode, crate::mir::MirInputStepMode::Vertex);
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn shader_contract_vertex_layout_reflection_json_matches_pinned_shape() {
    // JSON pin: the generic shader-contract path must serialize to the frozen
    // reflection shape. U1's audit proved the generic records equal the legacy
    // Triga-facts path field-for-field, so the pins below are unchanged.
    let vertex = crate::mir::MirKernelReflection::vertex_entry(
        crate::mir::MirFunctionId(7),
        "shader_plane_vertex",
        vec![
            crate::mir::MirVertexInputReflection {
                source_name: Some("position".to_owned()),
                location: 0,
                format: crate::mir::MirVertexInputFormat::Float32x3,
                step_mode: crate::mir::MirInputStepMode::Vertex,
                offset_bytes: 0,
                stride_bytes: 12,
                role: radix_mir::ShaderStageIoRole::Position,
            },
            crate::mir::MirVertexInputReflection {
                source_name: Some("normal".to_owned()),
                location: 1,
                format: crate::mir::MirVertexInputFormat::Float32x3,
                step_mode: crate::mir::MirInputStepMode::Vertex,
                offset_bytes: 0,
                stride_bytes: 12,
                role: radix_mir::ShaderStageIoRole::Other,
            },
            crate::mir::MirVertexInputReflection {
                source_name: Some("uv".to_owned()),
                location: 2,
                format: crate::mir::MirVertexInputFormat::Float32x2,
                step_mode: crate::mir::MirInputStepMode::Vertex,
                offset_bytes: 0,
                stride_bytes: 8,
                role: radix_mir::ShaderStageIoRole::Other,
            },
        ],
        vec![],
    )
    .expect("generic vertex reflection");
    let reflection = crate::mir::MirGpuReflection { kernels: vec![vertex], pipeline: None };
    let output = crate::Output::WgslText(crate::WgslTextOutput { code: String::new(), reflection: Some(reflection) });

    let json = output_reflection_json(&output)
        .expect("serialize GPU reflection JSON")
        .expect("GPU reflection JSON");
    let reflection: serde_json::Value = serde_json::from_str(&json).expect("reflection JSON");
    let kernel = &reflection["kernels"][0];
    let inputs = kernel["vertex_inputs"].as_array().expect("vertex inputs");
    let adapter = &kernel["launch"]["webgpu_adapter"];
    let vertex_layouts = adapter["vertex_buffer_layout_descriptors"]
        .as_array()
        .expect("WebGPU vertex buffer layouts");

    assert_eq!(kernel["shader_stage"], serde_json::json!("vertex"));
    assert_eq!(kernel["vertex_input_count"], serde_json::json!(3));
    assert_eq!(inputs[0]["source_name"], serde_json::json!("position"));
    assert_eq!(inputs[0]["location"], serde_json::json!(0));
    assert_eq!(inputs[0]["format"], serde_json::json!("float32x3"));
    assert_eq!(inputs[0]["step_mode"], serde_json::json!("vertex"));
    assert_eq!(inputs[0]["offset_bytes"], serde_json::json!(0));
    assert_eq!(inputs[0]["stride_bytes"], serde_json::json!(12));
    assert_eq!(inputs[1]["source_name"], serde_json::json!("normal"));
    assert_eq!(inputs[1]["location"], serde_json::json!(1));
    assert_eq!(inputs[1]["format"], serde_json::json!("float32x3"));
    assert_eq!(inputs[1]["offset_bytes"], serde_json::json!(0));
    assert_eq!(inputs[1]["stride_bytes"], serde_json::json!(12));
    assert_eq!(inputs[2]["source_name"], serde_json::json!("uv"));
    assert_eq!(inputs[2]["location"], serde_json::json!(2));
    assert_eq!(inputs[2]["format"], serde_json::json!("float32x2"));
    assert_eq!(inputs[2]["offset_bytes"], serde_json::json!(0));
    assert_eq!(inputs[2]["stride_bytes"], serde_json::json!(8));

    assert_eq!(adapter["vertex_buffer_layout_descriptor_count"], serde_json::json!(3));
    assert_eq!(adapter["vertex_buffer_layout_descriptor_indexes"], serde_json::json!([0, 1, 2]));
    assert_eq!(adapter["vertex_buffer_layout_descriptor_index_count"], serde_json::json!(3));
    assert_eq!(vertex_layouts[0]["buffer_index"], serde_json::json!(0));
    assert_eq!(vertex_layouts[0]["array_stride"], serde_json::json!(12));
    assert_eq!(vertex_layouts[0]["step_mode"], serde_json::json!("vertex"));
    assert_eq!(vertex_layouts[0]["attribute_indexes"], serde_json::json!([0]));
    assert_eq!(vertex_layouts[0]["attribute_index_count"], serde_json::json!(1));
    assert_eq!(vertex_layouts[0]["attribute_count"], serde_json::json!(1));
    assert_eq!(vertex_layouts[0]["source_name"], serde_json::json!("position"));
    assert_eq!(vertex_layouts[0]["attributes"][0]["shader_location"], serde_json::json!(0));
    assert_eq!(vertex_layouts[0]["attributes"][0]["format"], serde_json::json!("float32x3"));
    assert_eq!(vertex_layouts[0]["attributes"][0]["offset"], serde_json::json!(0));
    assert_eq!(vertex_layouts[0]["attributes"][0]["source_name"], serde_json::json!("position"));
    assert_eq!(vertex_layouts[1]["buffer_index"], serde_json::json!(1));
    assert_eq!(vertex_layouts[1]["array_stride"], serde_json::json!(12));
    assert_eq!(vertex_layouts[1]["attributes"][0]["shader_location"], serde_json::json!(1));
    assert_eq!(vertex_layouts[1]["attributes"][0]["format"], serde_json::json!("float32x3"));
    assert_eq!(vertex_layouts[1]["attributes"][0]["source_name"], serde_json::json!("normal"));
    assert_eq!(vertex_layouts[2]["buffer_index"], serde_json::json!(2));
    assert_eq!(vertex_layouts[2]["array_stride"], serde_json::json!(8));
    assert_eq!(vertex_layouts[2]["attributes"][0]["shader_location"], serde_json::json!(2));
    assert_eq!(vertex_layouts[2]["attributes"][0]["format"], serde_json::json!("float32x2"));
    assert_eq!(vertex_layouts[2]["attributes"][0]["source_name"], serde_json::json!("uv"));
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn emit_reflection_json_serializes_deferred_gpu_builtin_facts() {
    let mut types = crate::semantic::TypeTable::new();
    let u32_ty = crate::mir::MirType::semantic(
        types.sized_numeric(crate::semantic::Primitive::Numerus, crate::semantic::NumericWidth::U32),
    );
    let reflection = crate::mir::MirGpuReflection {
        kernels: vec![crate::mir::MirKernelReflection {
            fault_sites: Vec::new(),
            lane_range_binding: None,
            slab_origin_bindings: Vec::new(),
            function: crate::mir::MirFunctionId(0),
            provenance: crate::mir::MirKernelProvenance::MirFunction(crate::mir::MirFunctionId(0)),
            entry_name: "deferred_builtins".to_owned(),
            shader_stage: crate::mir::MirKernelShaderStage::Compute,
            resources: Vec::new(),
            bound_resources: Vec::new(),
            vertex_inputs: Vec::new(),
            varyings: Vec::new(),
            fragment_outputs: Vec::new(),
            builtin_inputs: vec![
                crate::mir::MirKernelBuiltinInputReflection {
                    source_local: crate::mir::MirLocalId(20),
                    source_name: None,
                    builtin: radix_types::MirKernelBuiltin::WorkgroupIdX,
                    axis: crate::mir::MirKernelAxis::X,
                    ty: u32_ty,
                    layout: crate::mir::MirScalarLayout::U32,
                },
                crate::mir::MirKernelBuiltinInputReflection {
                    source_local: crate::mir::MirLocalId(21),
                    source_name: None,
                    builtin: radix_types::MirKernelBuiltin::LocalInvocationIdY,
                    axis: crate::mir::MirKernelAxis::Y,
                    ty: u32_ty,
                    layout: crate::mir::MirScalarLayout::U32,
                },
                crate::mir::MirKernelBuiltinInputReflection {
                    source_local: crate::mir::MirLocalId(22),
                    source_name: None,
                    builtin: radix_types::MirKernelBuiltin::WorkgroupSizeZ,
                    axis: crate::mir::MirKernelAxis::Z,
                    ty: u32_ty,
                    layout: crate::mir::MirScalarLayout::U32,
                },
            ],
            called_builtins: vec![
                radix_types::MirKernelBuiltin::GlobalInvocationX,
                radix_types::MirKernelBuiltin::WorkgroupIdY,
            ],
            workgroup: crate::mir::MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: crate::mir::MirKernelDispatchSize { x: 1, y: 1, z: 1 },
            workgroup_count: crate::mir::MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        }],
        pipeline: None,
    };
    let output = crate::Output::WgslText(crate::WgslTextOutput { code: String::new(), reflection: Some(reflection) });

    let json = output_reflection_json(&output)
        .expect("serialize GPU reflection JSON")
        .expect("GPU reflection JSON");
    let reflection: serde_json::Value = serde_json::from_str(&json).expect("reflection JSON");
    let inputs = reflection["kernels"][0]["builtin_inputs"]
        .as_array()
        .expect("builtin inputs");

    assert_eq!(
        reflection["kernels"][0]["called_builtins"],
        serde_json::json!(["global_invocation_x", "workgroup_id_y"])
    );
    assert_eq!(inputs.len(), 3);
    assert_eq!(inputs[0]["builtin"], serde_json::json!("workgroup_id_x"));
    assert_eq!(inputs[0]["axis"], serde_json::json!("x"));
    assert_eq!(inputs[0]["source_local"], serde_json::json!(20));
    assert!(inputs[0].get("source_name").is_none());
    assert_eq!(inputs[0]["ty"]["semantic"], serde_json::json!(u32_ty.semantic_id().0));
    assert_eq!(inputs[0]["layout"], serde_json::json!("u32"));
    assert_eq!(inputs[1]["builtin"], serde_json::json!("local_invocation_id_y"));
    assert_eq!(inputs[1]["axis"], serde_json::json!("y"));
    assert_eq!(inputs[1]["source_local"], serde_json::json!(21));
    assert!(inputs[1].get("source_name").is_none());
    assert_eq!(inputs[1]["ty"]["semantic"], serde_json::json!(u32_ty.semantic_id().0));
    assert_eq!(inputs[1]["layout"], serde_json::json!("u32"));
    assert_eq!(inputs[2]["builtin"], serde_json::json!("workgroup_size_z"));
    assert_eq!(inputs[2]["axis"], serde_json::json!("z"));
    assert_eq!(inputs[2]["source_local"], serde_json::json!(22));
    assert!(inputs[2].get("source_name").is_none());
    assert_eq!(inputs[2]["ty"]["semantic"], serde_json::json!(u32_ty.semantic_id().0));
    assert_eq!(inputs[2]["layout"], serde_json::json!("u32"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn emit_reflection_json_rejects_non_gpu_output() {
    let result = compile_cli_source("main.fab", &la("incipit {}"), crate::codegen::Target::HirRust);

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = result.output.expect("rust output");
    assert!(
        output_reflection_json(&output)
            .expect("serialize reflection output")
            .is_none()
    );
}

#[test]
fn diagnostics_emit_can_render_semantic_record() {
    let result = compile_cli_source("main.fab", "missing", crate::codegen::Target::HirRust);

    assert!(!result.success());
    // em-u2 complete set: both the resolve-phase SEM001 (unknown_identifier)
    // and the lowering-phase SEM008 (undefined_name) records render.
    assert_eq!(result.diagnostics.len(), 2);
    let output = crate::diagnostics::render_expanded_diagnostics(&result.diagnostics);

    // ALLOW_DIAGNOSTIC_RENDER_TEXT: tool output rendered diagnostic contract
    assert!(output.contains("error[SEM001:unknown_identifier] analysis main.fab"));
    assert!(output.contains("error[SEM008:undefined_name] analysis main.fab"));
    assert!(output.contains("phase: analysis"));
    assert!(output.contains("source: \u{2068}missing\u{2069}"));
    assert!(!output.contains("help: "));
}

#[test]
#[cfg(feature = "hir-faber")]
fn emit_faber_target_returns_canonical_faber_output() {
    let result = compile_cli_source(
        "main.fab",
        &la("incipit {\n  nota \"ok\"\n}\n"),
        crate::codegen::Target::HirFaber,
    );

    assert!(result.success(), "{:?}", result.diagnostics);
    let Some(crate::Output::Faber(output)) = result.output else {
        panic!("expected Faber output");
    };
    assert!(output.code.contains("incipit {"));
    assert!(output.code.contains("nota \"ok\""));
    assert!(!output.code.contains("// Generated by radix"));
}

#[test]
#[cfg(feature = "hir-faber")]
fn emit_helper_uses_locale_pack_for_keyword_lexing() {
    let pack = zh_locale_pack();
    let result = compile_cli_source_with_locale_pack(
        "main.fab",
        "函数 salve() { }\n",
        crate::codegen::Target::HirFaber,
        Some(&pack),
        crate::codegen::OutputMode::Application,
        None,
    );

    assert!(result.success(), "{:?}", result.diagnostics);
    let Some(crate::Output::Faber(output)) = result.output else {
        panic!("expected Faber output");
    };
    // Phase 1 threading: the emit consults the reader pack, so localized source
    // re-emits in the localized surface (round-trip preserves the spelling).
    assert!(output.code.contains("函数 salve()"));
    assert!(!output.code.contains("functio"));
}

#[test]
#[cfg(feature = "hir-faber")]
fn emit_helper_rejects_undeclared_latin_keyword_in_grammar_position() {
    let pack = zh_locale_pack();
    let result = compile_cli_source_with_locale_pack(
        "main.fab",
        "functio salve() { }\n",
        crate::codegen::Target::HirFaber,
        Some(&pack),
        crate::codegen::OutputMode::Application,
        None,
    );

    // S2-U2 pack-authoritative gate: the zh-Hans pack does not declare the
    // Latin spelling `functio`, so it must NOT claim the function-declaration
    // slot. The word stays an ordinary identifier and the grammar-position use
    // fails normally (SEM001 undefined identifier) — no fallback event, no
    // LOCALE diagnostic.
    assert!(!result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.is_error() && diagnostic.code == Some("SEM001")),
        "undeclared spelling must fail as an ordinary undefined identifier: {:?}",
        result.diagnostics
    );
    assert!(result.diagnostics.iter().all(|diagnostic| {
        !diagnostic
            .code
            .is_some_and(|code| code.starts_with("LOCALE"))
    }));
}

#[test]
#[cfg(feature = "hir-faber")]
fn emit_faber_file_preserves_namespace_qualified_type_references() {
    let dir_td = temp_dir("faber-namespace-emit");
    let dir = dir_td.path();
    let types = dir.join("types.fab");
    let main = dir.join("main.fab");
    std::fs::write(&types, la("@ publica\ngenus Box {\n    varia textus value\n}\n")).expect("write types");
    std::fs::write(
        &main,
        la("importa ex \"./types\" T\n\nincipit {\n    sit T.Box box ← T.Box { value = \"ok\" }\n    nota box.value\n}\n"),
    )
    .expect("write main");

    let result = compile_cli_path(&main, false, crate::codegen::Target::HirFaber);

    assert!(result.success(), "{:?}", result.diagnostics);
    let Some(crate::Output::Faber(output)) = result.output else {
        panic!("expected Faber output");
    };
    assert!(output.code.contains("importa ex \"./types\" T"));
    assert!(output.code.contains("sit T.Box box"));
    assert!(!output.code.contains("genus Box"));
}

#[test]
fn emit_fmir_target_rejection_preserves_delegation_message() {
    let result = compile_cli_source("main.fab", &la("functio salve() {}"), crate::codegen::Target::MirFmir);

    assert!(!result.success());
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == Some("CODEGEN001"))
        .expect("FMIR emit rejection should surface as codegen diagnostic");
    let rendered = crate::diagnostics::render_plain(diagnostic);

    // ALLOW_DIAGNOSTIC_RENDER_TEXT: this test verifies the plain rendered delegation text.
    assert!(rendered.contains("CODEGEN001"), "{rendered}");
    assert!(
        rendered.contains("fmir-text package images must be built through"),
        "{rendered}"
    );
    assert!(rendered.contains("faber build --target fmir-text"), "{rendered}");
}

#[test]
fn mir_output_for_source_returns_deterministic_text() {
    let output = mir_output_for_source("main.fab", &la("functio saluta() {}")).expect("MIR output");

    assert_eq!(
        output,
        "\
function f0 -> ty#5 {
  bb0:
    return
}
"
    );
}

#[test]
fn mir_output_for_source_returns_error_for_invalid_syntax() {
    let result = mir_output_for_source("bad.fab", "si verum tacet");
    assert!(result.is_err(), "invalid source should produce error");
    let errors = result.expect_err("error expected");
    assert!(!errors.is_empty(), "should report at least one error");
}

#[test]
fn mir_output_reports_unknown_library_module_for_unresolved_norma_import() {
    let (_td, path) = temp_fab(
        "mir-unknown-norma-module",
        &la("importa ex \"norma:nonexistent\" nonexistent\n\nincipit {\n    nota \"unreachable\"\n}\n"),
    );
    let source = std::fs::read_to_string(&path).expect("read temp fab");

    let errors = mir_output_for_source(&path, &source).expect_err("unresolved norma import should be rejected");
    let identity = errors
        .iter()
        .find_map(|error| error.strip_prefix("error[")?.split_once("]: "))
        .map(|(identity, _)| identity)
        .expect("error output should have a diagnostic identity header");
    assert_eq!(identity, "PKG001:unknown_library_module");
}

#[test]
fn mir_output_for_control_flow_source_returns_deterministic_text() {
    let output = mir_output_for_source(
        "main.fab",
        &la("functio signum(numerus n) → numerus { si n ≻ 0 ergo redde n redde 0 }"),
    )
    .expect("MIR output");

    assert_eq!(
        output,
        "\
function f0 -> ty#1 {
  params:
    _0: ty#1
  locals:
    let _0: ty#1
  temps:
    %0: ty#3
    %1: ty#1
  bb0:
    %0 = _0 > const int 0: ty#3
    branch %0 bb1 bb2
  bb1:
    return _0
  bb2:
    %1 = const int 0: ty#1
    return %1
}
"
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_fragment_entry_rejects_annotation_payload() {
    let source = r#"+++
locale = "la"
+++
@ fragment { mode = "x" }
functio bad_fragment() → vacuum {
}
"#;
    let result = compile_cli_source("fragment-payload.fab", source, crate::codegen::Target::MirWgsl);

    assert!(!result.success(), "fragment annotation with payload must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("mir_wgsl_fragment_source_annotation_payload")),
        "expected mir_wgsl_fragment_source_annotation_payload, got {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_fragment_entry_rejects_parameters() {
    let source = r#"+++
locale = "la"
+++
@ fragment
functio bad_fragment(numerus x) → vacuum {
}
"#;
    let result = compile_cli_source("fragment-params.fab", source, crate::codegen::Target::MirWgsl);

    assert!(!result.success(), "fragment entry with parameters must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("mir_wgsl_fragment_source_params")),
        "expected mir_wgsl_fragment_source_params, got {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_fragment_entry_rejects_non_vacuum_return() {
    let source = r#"+++
locale = "la"
+++
@ fragment
functio bad_fragment() → numerus {
    redde 1
}
"#;
    let result = compile_cli_source("fragment-return.fab", source, crate::codegen::Target::MirWgsl);

    assert!(!result.success(), "fragment entry with non-vacuum return must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("mir_wgsl_fragment_source_return")),
        "expected mir_wgsl_fragment_source_return, got {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_fragment_entry_rejects_error_type() {
    let source = r#"+++
locale = "la"
+++
@ fragment
functio bad_fragment() → vacuum ⇥ textus {
}
"#;
    let result = compile_cli_source("fragment-error.fab", source, crate::codegen::Target::MirWgsl);

    assert!(!result.success(), "fragment entry with error type must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("mir_wgsl_fragment_source_error_type")),
        "expected mir_wgsl_fragment_source_error_type, got {:?}",
        result.diagnostics
    );
}

#[test]
fn wgsl_text_source_fragment_entry_accepts_non_empty_body() {
    // Fragment body rejection gate removed in U4; fragment body lowering is deferred
    // to a follow-up unit but the gate no longer blocks compilation.
    let source = r"
@ fragment
functio ok_fragment() → vacuum {
    tacet
}
";
    let result = compile_cli_source("fragment-body.fab", source, crate::codegen::Target::MirWgsl);

    // Fragment body lowering is deferred; non-empty body is no longer rejected.
    // The fragment path may still fail on other grounds (e.g. missing layout facts),
    // but the mir_wgsl_fragment_source_body rejection gate is gone.
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("mir_wgsl_fragment_source_body")),
        "mir_wgsl_fragment_source_body rejection gate must be removed: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "mir-wgsl")]
fn assert_wgsl_body_reads_uv(fixture: &str, name: &str, source: &str) {
    let (_td, file) = temp_fab(name, source);
    let path = PathBuf::from(&file);
    let result = compile_cli_path(&path, false, crate::codegen::Target::MirWgsl);
    let _ = std::fs::remove_file(&path);

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = result.output.expect("wgsl output");
    let crate::Output::WgslText(wgsl_output) = &output else {
        panic!("expected WGSL output");
    };
    assert!(
        wgsl_output.code.contains("input.uv"),
        "expected input.uv in WGSL:\n{}",
        wgsl_output.code
    );
    validate_wgsl_with_naga(&wgsl_output.code, fixture);
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_fragment_body_reads_uv() {
    assert_wgsl_body_reads_uv(
        "shader-contract-fragment-uv",
        "fragment-reads-uv",
        include_str!("../fixtures/shader-contract-fragment-uv.fab"),
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_vertex_body_reads_uv() {
    assert_wgsl_body_reads_uv(
        "shader-contract-vertex-uv",
        "vertex-reads-uv",
        include_str!("../fixtures/shader-contract-vertex-uv.fab"),
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_source_fragment_entry_rejects_multiple_entries() {
    let source = r#"+++
locale = "la"
+++
@ fragment
functio first_fragment() → vacuum {
}

@ fragment
functio second_fragment() → vacuum {
}
"#;
    let result = compile_cli_source("fragment-count.fab", source, crate::codegen::Target::MirWgsl);

    assert!(!result.success(), "multiple fragment entries must fail closed");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("mir_wgsl_fragment_source_entry_count")),
        "expected mir_wgsl_fragment_source_entry_count, got {:?}",
        result.diagnostics
    );
}

// ── U3.2 FHIR emit tests ─────────────────────────────────────────────────

#[test]
#[cfg(feature = "hir-fhir")]
fn emit_fhir_serializes_valid_faber_to_fhir_bytes() {
    // Prove basic FHIR serialization: analyze a valid source, construct
    // HirArtifact via from_analyzed, serialize via postcard, and verify
    // the wire format starts with the schema version header.
    let source = "incipit {\n  nota \"hello from fhir\"\n}\n";
    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_latin_locale()
            .with_target(crate::codegen::Target::HirFhir)
            .with_dev_locale_root(),
    );
    let unit = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default())
        .expect("analysis should succeed");

    let artifact = crate::hir::serialize::from_analyzed(&unit, "test_hash", "test.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("postcard serialization should succeed");

    // Schema version serializes as a postcard varint (single byte for <128).
    assert!(!bytes.is_empty(), "serialized artifact should be non-empty");
    assert_eq!(
        bytes[0],
        radix_hir_fhir::SCHEMA_VERSION as u8,
        "FHIR artifact must start with schema version; first byte: {:02x?}",
        bytes[0]
    );
}

#[test]
#[cfg(feature = "hir-fhir")]
fn emit_fhir_round_trip_hello_world() {
    // End-to-end FHIR round-trip: analyze → serialize → deserialize → verify.
    let source = include_str!("../../tests/fixtures/hir_artifact/hello-world.fab");
    // Peel frontmatter: leading `//!` / `#` comment lines at the top of the fixture.
    let peeled = crate::driver::peel_raw_source("hello-world.fab", source).expect("fixture frontmatter should peel");

    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_latin_locale()
            .with_target(crate::codegen::Target::HirFhir)
            .with_dev_locale_root(),
    );
    let unit = crate::driver::analyze_source(&session, "hello-world.fab", peeled.body, AnalyzeRequest::default())
        .expect("hello-world fixture should analyze");

    let artifact = crate::hir::serialize::from_analyzed(&unit, "hello-sha256", "hello-world.fab");
    let bytes = postcard::to_allocvec(&artifact).expect("postcard serialization should succeed");

    // Deserialize round-trip
    let loaded = crate::hir::artifact::load_fhir(&bytes).expect("hello-world FHIR artifact should load");

    // Prove the loaded unit produces identical codegen output for Faber target.
    let pack = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&pack);
    let original_output = crate::codegen::generate_from_analyzed(crate::codegen::Target::HirFaber, &unit, &surface)
        .expect("original Faber codegen should succeed");
    let loaded_output = crate::codegen::generate_from_analyzed(crate::codegen::Target::HirFaber, &loaded, &surface)
        .expect("loaded Faber codegen should succeed");

    let original_code = match original_output {
        crate::Output::Faber(o) => o.code,
        _ => panic!("expected Faber output"),
    };
    let loaded_code = match loaded_output {
        crate::Output::Faber(o) => o.code,
        _ => panic!("expected Faber output"),
    };
    assert_eq!(
        original_code, loaded_code,
        "FHIR round-trip: Faber codegen output must be byte-identical"
    );
}

fn check_locale002_spellings(result: &crate::services::CheckDiagnosticsResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some("LOCALE002"))
        .filter_map(|d| {
            d.args
                .iter()
                .find(|arg| arg.name == "spelling")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

fn check_with_installed_pack(locale: &str, source: &str) -> crate::services::CheckDiagnosticsResult {
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .expect("installed pack should load");
    check_diagnostics_for_source_with_locale_pack("main.fab", source, false, Some(&pack), None)
}

#[test]
fn check_valid_identifier_near_keyword_draws_no_locale002() {
    let result = check_with_installed_pack("en", "main {\n  let int clamp ← 1\n  print clamp\n}\n");

    assert!(result.success, "diagnostics: {:?}", result.diagnostics);
    assert!(
        check_locale002_spellings(&result).is_empty(),
        "a valid identifier must not draw LOCALE002: {:?}",
        result.diagnostics
    );
}

#[test]
fn check_misspelled_name_at_resolve_error_keeps_locale002() {
    let result = check_with_installed_pack("en", "main {\n  let int clamp ← 1\n  print clamq\n}\n");

    assert!(!result.success);
    assert_eq!(
        check_locale002_spellings(&result),
        vec!["clamq".to_string()],
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn check_misspelled_keyword_at_parse_error_keeps_locale002() {
    // `retorn` is one edit from the English `return`; the parser rejects it.
    let result = check_with_installed_pack("en", "main {\n    print 1 retorn\n}\n");

    assert!(!result.success);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.phase == crate::diagnostics::DiagnosticPhase::Parse),
        "expected a parse error: {:?}",
        result.diagnostics
    );
    assert_eq!(
        check_locale002_spellings(&result),
        vec!["retorn".to_string()],
        "diagnostics: {:?}",
        result.diagnostics
    );
}

fn wrong_locale_diagnostic(result: &CheckDiagnosticsResult) -> Option<usize> {
    result.diagnostics.iter().position(|diagnostic| {
        diagnostic.code == Some("LOCALE002")
            && radix_semantic::arg_value(&diagnostic.args, "issue") == Some("source_reads_as_other_locale")
            && radix_semantic::arg_value(&diagnostic.args, "detected") == Some("la")
    })
}

fn assert_wrong_locale_reported_first(name: &str, source: &str) {
    let result = check_diagnostics_for_source(name, source, false);
    let reported = wrong_locale_diagnostic(&result)
        .unwrap_or_else(|| panic!("no detected-locale diagnostic for {name}: {:?}", result.diagnostics));
    let first_unknown = result
        .diagnostics
        .iter()
        .position(|diagnostic| diagnostic.code == Some("SEM001"))
        .expect("unknown identifier error");
    assert!(reported < first_unknown, "diagnostics: {:?}", result.diagnostics);
    assert!(!result.diagnostics[reported].is_error(), "detection must not be an error");
}

#[test]
fn check_wrong_locale_source_reports_detected_locale_on_stdin_route() {
    assert_wrong_locale_reported_first("<stdin>", "incipit { nota 1 }");
}

#[test]
fn check_wrong_locale_source_reports_detected_locale_on_file_route() {
    let (_dir, path) = temp_fab("wrong-locale", "incipit { nota 1 }");
    let source = std::fs::read_to_string(&path).expect("read temp fab");
    assert_wrong_locale_reported_first(&path, &source);
}

#[test]
fn check_wrong_locale_source_reports_detected_locale_on_command_route() {
    assert_wrong_locale_reported_first("command-line", "incipit { nota 1 }");
}

#[test]
fn check_wrong_locale_source_reports_locales002_once_and_first() {
    // The reading note must not be doubled by a keyword-level LOCALE002 for
    // the same file, and it must precede the errors it explains.
    let result = check_diagnostics_for_source("once.fab", "incipit { nota 1 }", false);
    assert!(!result.success, "diagnostics: {:?}", result.diagnostics);
    let notes: Vec<usize> = result
        .diagnostics
        .iter()
        .enumerate()
        .filter_map(|(index, diagnostic)| (diagnostic.code == Some("LOCALE002")).then_some(index))
        .collect();
    assert_eq!(notes.len(), 1, "diagnostics: {:?}", result.diagnostics);
    assert_eq!(
        radix_semantic::arg_value(&result.diagnostics[notes[0]].args, "issue"),
        Some("source_reads_as_other_locale")
    );
    let first_error = result
        .diagnostics
        .iter()
        .position(crate::diagnostics::Diagnostic::is_error)
        .expect("the wrong-locale read must still fail");
    assert!(notes[0] < first_error, "diagnostics: {:?}", result.diagnostics);
}

/// A source whose `+++` frontmatter declares a locale draws no LOCALE002
/// wrong-locale advisory: the note must not claim the source has no declared
/// locale when it does. Asserts code and issue name — never message text.
#[test]
fn check_declared_locale_source_draws_no_locales002_note() {
    let source = "+++\nlocale = \"en\"\n+++\n\nincipit { nota 1 }";
    let result = check_diagnostics_for_source("declared.fab", source, false);
    assert!(!result.success, "diagnostics: {:?}", result.diagnostics);
    let notes: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.code == Some("LOCALE002")
                && radix_semantic::arg_value(&diagnostic.args, "issue") == Some("source_reads_as_other_locale")
        })
        .collect();
    assert!(notes.is_empty(), "diagnostics: {:?}", result.diagnostics);
}

// ── single-file check/verify resolve frontmatter locales through the config ──
//
// `faber check` / `lex` / `parse` / `hir` / `cli-ir` / `verify` resolved a
// declared `+++ locale = "…" +++` through a default config's discovered
// workspace root only. The `_with_config` services take the
// caller's root/loader, and declaration detection no longer depends on the
// declared pack being loadable.

fn loader_only_config() -> crate::driver::Config {
    let en =
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en")).expect("en pack");
    let la =
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la")).expect("la pack");
    let mut config = crate::driver::Config::default();
    config.locale_root = None;
    config.with_locale_pack_loader(crate::driver::LocalePackLoader::new(move |locale_id| match locale_id {
        "en" => Ok(en.clone()),
        "la" => Ok(la.clone()),
        other => Err(format!("no pack for locale '{other}'")),
    }))
}

#[test]
fn check_resolves_frontmatter_locale_through_config_loader() {
    let source = "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio salve() { }\n";
    let result = crate::services::check_diagnostics_for_source_with_config(
        "la.fab",
        source,
        false,
        None,
        None,
        &loader_only_config(),
    );
    assert!(
        result.success,
        "a Latin frontmatter file must check with no workspace root: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_fails_closed_on_unresolvable_frontmatter_locale() {
    let source = "+++\nlocale = \"zh-Hant\"\n+++\n@ publica\nfunctio salve() { }\n";
    let result = crate::services::check_diagnostics_for_source_with_config(
        "la.fab",
        source,
        false,
        None,
        None,
        &loader_only_config(),
    );
    assert!(!result.success, "an unloadable declared locale must fail the check");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE003") && d.issue() == Some("load_failed")),
        "expected LOCALE003 load_failed, got {:?}",
        result.diagnostics
    );
}

#[test]
fn source_declares_locale_is_independent_of_pack_loadability() {
    // Declaration is a frontmatter fact: an unloadable locale id still
    // declares, and only the absence of the key does not.
    assert!(crate::services::source_declares_locale(
        "a.fab",
        "+++\nlocale = \"no-such-locale\"\n+++\nmain { }\n"
    ));
    assert!(crate::services::source_declares_locale(
        "a.fab",
        "+++\nlocale = 7\n+++\nmain { }\n"
    ));
    assert!(!crate::services::source_declares_locale("a.fab", "main { }\n"));
    assert!(!crate::services::source_declares_locale(
        "a.fab",
        "+++\nother = \"la\"\n+++\nmain { }\n"
    ));
}

#[test]
fn verify_resolves_frontmatter_locale_through_config_loader() {
    let source = "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio salve() { }\n";
    let result = crate::services::verify_source_with_config("la.fab", source, None, &loader_only_config());
    // A clean Latin read under the declared locale produces no diagnostics at
    // all — not merely none of the lost-locale codes — and the aspect
    // machinery still ran (the coverage report is present).
    assert!(
        result.diagnostics.is_empty(),
        "verify must read the Latin source cleanly under its declared locale: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        result.coverage.is_some(),
        "verify must still produce its aspect coverage report"
    );
}
