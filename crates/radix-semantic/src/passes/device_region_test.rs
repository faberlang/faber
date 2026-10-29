//! Coverage for the checked device region (`do device { … } catch e { … }`,
//! la `fac machina`): the legality rows, the host-control rulings, and the
//! imported `@ nucleum` contract reader.
//!
//! Region fixtures use the English surface so they read like the corpus.
//! Every rejection asserts the structured `issue`, never rendered prose.

use std::collections::BTreeMap;

use crate::PassConfig;
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceBodyChannel, InterfaceCallable,
    InterfaceDeviceContract, InterfaceDeviceRole, InterfaceDeviceVisibility, InterfaceParam,
    InterfaceTypeSnapshot,
};
use crate::locale::LocalePack;
use crate::passes::visibility_annotation::VisibilityTier;
use crate::test_support::{self, AnalyzedFixture, analyze_source_with_config};
use crate::{Primitive, SemanticError};
use radix_target::Target;

/// Kernels a region may call, plus the host helper it may not.
const PREAMBLE: &str = r"
@ kernel
@ public
fn bump<size N>(tensor<f32, [N]> dom, mut tensor<f32, [N]> t, i32 step) → void {
    for from dom at [i] thread lane let v {
        t[[i]] ← v + (step ↦ f32)
    }
}

fn host_helper(int x) → int {
    return x + 1
}
";

fn en_config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    config
}

/// A `main` whose body is `body`, with `t` and `z` tensors in scope.
fn region_program(body: &str) -> String {
    format!(
        "{PREAMBLE}\nmain {{\n    const size N ← 4\n    mut tensor<f32, [N]> t ← tf32[N] {{}}\n    \
         let tensor<f32, [N]> z ← tf32[N] {{}}\n{body}\n}}\n"
    )
}

fn analyze(source: &str) -> AnalyzedFixture {
    analyze_source_with_config(source, en_config())
}

fn issues(unit: &AnalyzedFixture) -> Vec<&str> {
    unit.diagnostics
        .iter()
        .filter(|err| err.is_error())
        .filter_map(|err| test_support::issue(err))
        .collect()
}

fn arg<'a>(err: &'a SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn assert_region_accepts(body: &str) {
    let unit = analyze(&region_program(body));
    assert!(unit.is_ok(), "diagnostics: {:?}", unit.diagnostics);
}

fn assert_region_rejects(body: &str, expected: &str) {
    let unit = analyze(&region_program(body));
    assert!(
        issues(&unit).contains(&expected),
        "expected {expected:?}, got: {:?}",
        unit.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Accepted regions
// ---------------------------------------------------------------------------

#[test]
fn a_region_of_kernel_calls_and_captured_loops_is_legal() {
    assert_region_accepts(
        r#"    do device {
        for range 0‥10 let k {
            bump(z, mut t, k ↦ i32)
        }
    }
    catch err {
        print "failed"
    }"#,
    );
}

#[test]
fn a_region_without_a_handler_is_legal() {
    assert_region_accepts("    do device {\n        bump(z, mut t, 1)\n    }");
}

#[test]
fn a_do_device_while_tail_keeps_host_control() {
    // The `while` condition is host control and sits outside the region, so a
    // device read in it is not a region refusal.
    let unit = analyze(&region_program(
        "    do device {\n        bump(z, mut t, 1)\n    } while t[[0]] ≺ 3.0",
    ));
    assert!(
        !issues(&unit).contains(&"nucleum_region_device_read_not_built"),
        "the tail condition is host control, got: {:?}",
        unit.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[test]
fn a_nested_region_is_refused() {
    assert_region_rejects(
        "    do device {\n        do device {\n            bump(z, mut t, 1)\n        }\n    }",
        "fac_machina_nested",
    );
}

#[test]
fn a_handler_inside_a_region_is_refused() {
    assert_region_rejects(
        "    do device {\n        do {\n            bump(z, mut t, 1)\n        } catch e {\n            print \"x\"\n        }\n    }",
        "nucleum_recovery",
    );
}

#[test]
fn break_crossing_out_of_a_region_is_refused() {
    assert_region_rejects(
        "    for range 0‥3 let j {\n        do device {\n            bump(z, mut t, 1)\n            if j ≡ 1 then break\n        }\n    }",
        "nucleum_block_exit_crosses_boundary",
    );
}

#[test]
fn return_out_of_a_region_is_refused() {
    let unit = analyze(&format!(
        "{PREAMBLE}\nfn run() → int {{\n    const size N ← 4\n    mut tensor<f32, [N]> t ← tf32[N] {{}}\n    \
         let tensor<f32, [N]> z ← tf32[N] {{}}\n    do device {{\n        bump(z, mut t, 1)\n        return 1\n    }}\n    return 0\n}}\n"
    ));
    assert!(
        issues(&unit).contains(&"nucleum_block_exit_crosses_boundary"),
        "got: {:?}",
        unit.diagnostics
    );
}

#[test]
fn a_direct_lane_loop_is_refused() {
    assert_region_rejects(
        "    do device {\n        for from z at [i] thread lane let v {\n            t[[i]] ← v\n        }\n    }",
        "nucleum_region_lane_loop_not_built",
    );
}

#[test]
fn a_tensor_allocation_is_refused() {
    assert_region_rejects(
        "    do device {\n        let tensor<f32, [N]> w ← tf32[N] {}\n        bump(w, mut t, 1)\n    }",
        "nucleum_region_alloc_not_built",
    );
}

#[test]
fn a_condition_on_a_device_read_is_refused() {
    assert_region_rejects(
        "    do device {\n        bump(z, mut t, 1)\n        if t[[0]] ≻ 0.5 then bump(z, mut t, 2)\n    }",
        "nucleum_region_device_read_not_built",
    );
}

#[test]
fn a_data_dependent_while_on_device_data_is_refused_by_name() {
    assert_region_rejects(
        "    do device {\n        while t[[0]] ≺ 1.0 {\n            bump(z, mut t, 1)\n        }\n    }",
        "nucleum_region_device_read_not_built",
    );
}

#[test]
fn a_device_read_as_a_kernel_scalar_argument_is_refused() {
    assert_region_rejects(
        "    do device {\n        bump(z, mut t, (⌊t[[0]]⌋ ↦ i32))\n    }",
        "nucleum_region_device_read_not_built",
    );
}

#[test]
fn a_host_effect_in_a_region_is_refused() {
    assert_region_rejects(
        "    do device {\n        print \"x\"\n        bump(z, mut t, 1)\n    }",
        "nucleum_host_effect",
    );
}

#[test]
fn a_host_function_call_in_a_region_is_refused() {
    assert_region_rejects(
        "    do device {\n        let int q ← host_helper(3)\n        bump(z, mut t, 1)\n    }",
        "nucleum_host_call",
    );
}

#[test]
fn a_wide_integer_capture_is_admitted() {
    // Ruling 2026-10-28: 64-bit integers are kernel values. The capture is a
    // per-target capability fact in the requirement report, not a check error.
    assert_region_accepts(
        "    let int steps ← 5\n    do device {\n        for range 0‥steps let k {\n            bump(z, mut t, k ↦ i32)\n        }\n    }",
    );
}

// ---------------------------------------------------------------------------
// Ordinary `fac omnia` is unchanged
// ---------------------------------------------------------------------------

#[test]
fn an_omnia_block_is_not_a_region() {
    // `do all` is the host-resident deferral and carries no device legality.
    let unit = analyze(&region_program(
        "    do all {\n        let int q ← host_helper(3)\n        bump(z, mut t, 1)\n    }\n    catch e {\n        print \"x\"\n    }",
    ));
    assert!(
        !issues(&unit).contains(&"nucleum_host_call"),
        "`do all` must not be checked as a device region, got: {:?}",
        unit.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Imported `@ nucleum` contract (the resolver reader)
// ---------------------------------------------------------------------------

fn scalar_param() -> InterfaceParam {
    InterfaceParam {
        ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
        mode: crate::SemanticParamMode::Owned,
        optional: false,
        sponte: false,
        axis: false,
    }
}

fn callable(device: Option<InterfaceDeviceContract>) -> InterfaceCallable {
    InterfaceCallable {
        type_params: Vec::new(),
        params: vec![scalar_param()],
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
        err: None,
        is_async: false,
        is_generator: false,
        tensor_edge_policy_requirements: Vec::new(),
        device,
    }
}

fn kernel_contract() -> InterfaceDeviceContract {
    InterfaceDeviceContract {
        role: InterfaceDeviceRole::Kernel,
        visibility: InterfaceDeviceVisibility::Public,
        body: InterfaceBodyChannel::Source,
    }
}

fn analyze_imported(body: &str) -> AnalyzedFixture {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "kern".to_owned(),
        kind: FileExportKind::Function(callable(Some(kernel_contract()))),
    });
    interface.insert(FileExport {
        name: "plain".to_owned(),
        kind: FileExportKind::Function(callable(None)),
    });
    let tiers = BTreeMap::from([
        ("kern".to_owned(), VisibilityTier::Publica),
        ("plain".to_owned(), VisibilityTier::Publica),
    ]);
    let source = format!("importa ex \"./lib\" lib\n\nincipit {{\n{body}\n}}\n");
    test_support::analyze_source_with_import_contract_la(
        &source,
        BTreeMap::from([("lib".to_owned(), tiers)]),
        BTreeMap::from([("lib".to_owned(), interface)]),
    )
}

#[test]
fn an_imported_public_kernel_is_admitted_in_a_region() {
    let unit = analyze_imported("    fac machina {\n        lib.kern(1)\n    }");
    assert!(
        !issues(&unit).contains(&"nucleum_host_call"),
        "an imported `@ nucleum` callee is a kernel, got: {:?}",
        unit.diagnostics
    );
}

#[test]
fn an_imported_host_function_is_refused_in_a_region() {
    let unit = analyze_imported("    fac machina {\n        lib.plain(1)\n    }");
    let err = unit
        .diagnostics
        .iter()
        .find(|err| err.issue() == Some("nucleum_host_call"))
        .unwrap_or_else(|| panic!("missing nucleum_host_call in {:?}", unit.diagnostics));
    assert_eq!(arg(err, "callee"), Some("lib.plain"));
    assert_eq!(arg(err, "reason"), Some("not_a_kernel"));
}
