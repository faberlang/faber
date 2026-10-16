//! CDC-1: consumer-local device-caller composition at the package seam.
//!
//! A consumer-local device function (entry unit or sibling unit) that calls
//! an imported device instance composes in the one existing package-seam
//! pass between shape finalization and the `ValidatedMir` rebuild — the
//! leaf-tail union of the consumer units' own device roles/launch entries
//! with the imported routes (design ruling 2026-08-27, audit `25749061`).
//!
//! Both-path parity (done_when (h)): the composition outcome is proven
//! through both analysis entry paths that can produce device facts — the
//! library-interface path (`library.rs`, cached `program_export_surface`,
//! the `b73f39671` mirror-repair seam) where the imported instance arrives
//! as a KPC-WIRE route, and the package-own path (`analyze.rs`,
//! `export_surfaces_by_path`) where a package sibling's exported kernel
//! arrives through the source rewrite join. Named boundary: linked-library
//! units' own local device callers and template-free cross-unit device
//! calls are not package units and never reach the leaf compose (their
//! calls fail closed downstream, unchanged from the pre-CDC-1 seam). A
//! second boundary, pre-existing and untouched by the union: a linked
//! library's generic template whose body calls another generic template
//! collapses its shape params at the library merge
//! (`invalid_mir_call_argument_type_mismatch` before composition), so
//! imported depth chains cannot be built from source today — the budget
//! identity is proven with consumer-local inputs at the seam API instead
//! (`package_instantiate_test.rs`).

use super::*;

const COMPOSE_MATH_LIB: &str = r#"+++
locale = "en"
+++
@ kernel
@ public
fn add<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}
"#;

const CONSUMER_KERNEL_CALLER: &str = r#"+++
locale = "en"
+++
import from "composelib:math" math

@ kernel
@ public
fn probe_add(tensor<f32, [4, 8]> a, tensor<f32, [4, 8]> b) → tensor<f32, [4, 8]> {
    return math.add(a, b)
}

fn main() → void {
    print "ok"
}
"#;

const CONSUMER_PRIVATE_HELPER_CALLER: &str = r#"+++
locale = "en"
+++
import from "composelib:math" math

@ kernel
fn solo_helper(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {
    return math.add(a, b)
}

# Rule 12.7: host code may not call the private `solo_helper`, so the host
# reaches it through this public kernel wrapper.
@ kernel
@ public
fn solo_entry(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {
    return solo_helper(a, b)
}

fn main() → void {
    let _ a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    let _ b ← [[10.0, 20.0], [30.0, 40.0]] ↦ tensor<f32, [2, 2]>
    let _ r ← solo_entry(a, b)
    print "ok"
}
"#;

const CONSUMER_HOST_CALLER: &str = r#"+++
locale = "en"
+++
import from "composelib:math" math

fn main() → void {
    let _ a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    let _ b ← [[10.0, 20.0], [30.0, 40.0]] ↦ tensor<f32, [2, 2]>
    let _ r ← math.add(a, b)
    print "ok"
}
"#;

/// The package-own-path twin: the same exported public generic kernel
/// provided by a package SIBLING unit (analyzed by `analyze.rs` with
/// `export_surfaces_by_path` admissions) instead of a linked library.
const SIBLING_MATH_LIB: &str = r#"+++
locale = "en"
+++
@ kernel
@ public
fn add<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}
"#;

const SIBLING_CONSUMER_KERNEL_CALLER: &str = r#"+++
locale = "en"
+++
import from "./math" math

@ kernel
@ public
fn probe_add(tensor<f32, [4, 8]> a, tensor<f32, [4, 8]> b) → tensor<f32, [4, 8]> {
    return math.add(a, b)
}

fn main() → void {
    print "ok"
}
"#;

const CYCLE_MATH_LIB: &str = r#"+++
locale = "en"
+++
@ kernel
@ public
fn ping<size M, size N>(tensor<f32, [M, N]> a) → tensor<f32, [M, N]> {
    let tensor<f32, [M, N]> out ← ping(a)
    return out
}
"#;

const CONSUMER_CYCLE_CALLER: &str = r#"+++
locale = "en"
+++
import from "composelib:math" math

@ kernel
@ public
fn probe_ping(tensor<f32, [4, 8]> a) → tensor<f32, [4, 8]> {
    return math.ping(a)
}

fn main() → void {
    print "ok"
}
"#;

/// A `<size>`-generic exported public kernel chain deeper than the
/// documented composition depth budget (16): `step_<N>` calls
/// `step_<N-1>` down to `step_0`.
fn deep_chain_library(depth: usize) -> String {
    let mut source = String::from("+++\nlocale = \"en\"\n+++\n");
    for level in (0..=depth).rev() {
        if level == 0 {
            source.push_str("@ kernel\n@ public\nfn step_0<size M>(tensor<f32, [M]> a) → tensor<f32, [M]> {\n    return a\n}\n");
        } else {
            source.push_str(&format!(
                "@ kernel\n@ public\nfn step_{level}<size M>(tensor<f32, [M]> a) → tensor<f32, [M]> {{\n    let tensor<f32, [M]> out ← step_{}(a)\n    return out\n}}\n",
                level - 1
            ));
        }
    }
    source
}

const CONSUMER_BUDGET_CALLER: &str = r#"+++
locale = "en"
+++
import from "composelib:math" math

@ kernel
@ public
fn probe_deep(tensor<f32, [4]> a) → tensor<f32, [4]> {
    return math.step_24(a)
}

fn main() → void {
    print "ok"
}
"#;

fn write_compose_package(dir: &Path, library: &str, consumer: &str) -> PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library_root = libhome.join("composelib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library_root.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "consumer-device-composition"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
composelib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library_root.display();
    let lock = format!(
        "[[package]]\nname = \"composelib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library_root.join("src/math.fab"), library).expect("write library module");
    fs::write(src.join("main.fab"), consumer).expect("write main.fab");
    src.join("main.fab")
}

fn write_sibling_package(dir: &Path, sibling: &str, consumer: &str) -> PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "consumer-device-composition-sibling"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write faber.toml");
    fs::write(src.join("math.fab"), sibling).expect("write sibling module");
    fs::write(src.join("main.fab"), consumer).expect("write main.fab");
    src.join("main.fab")
}

fn function_by_name<'a>(lowered: &'a LoweredMirUnit<'_>, name: &str) -> &'a MirFunction {
    lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| {
            function
                .name
                .is_some_and(|symbol| lowered.interner.resolve(symbol) == name)
        })
        .unwrap_or_else(|| panic!("`{name}` must exist in the merged program"))
}

/// Count every `Call`/`TryCall` site in one function — the composition proof
/// helper: a device caller whose only call was the imported-instance call
/// must have zero sites after the seam composed it.
fn call_site_count(function: &MirFunction) -> usize {
    let mut sites = 0;
    for block in &function.blocks {
        for statement in &block.statements {
            if matches!(statement.kind, MirStatementKind::Call { .. }) {
                sites += 1;
            }
        }
        if matches!(block.terminator.kind, MirTerminatorKind::TryCall { .. }) {
            sites += 1;
        }
    }
    sites
}

/// The imported-instance composition outcome through the library-interface
/// analysis path (done_when (a), (d), (h) library leg): the consumer-local
/// kernel's imported call composes at the seam, the imported public Kernel
/// instance registers launchable and stays a program symbol (the union
/// prevents pruning), and no call site survives in the consumer caller.
#[test]
fn library_import_composes_consumer_local_kernel_caller() {
    let dir = crate::test_support::test_temp_dir("cdc-library-compose");
    let entry = write_compose_package(dir.path(), COMPOSE_MATH_LIB, CONSUMER_KERNEL_CALLER);
    let config = Config::default().with_library_home(dir.join("libhome"));

    // CDC-3: the consumer's local entry roles for the merged device-role
    // map (the same derivation the single-unit driver performs); package
    // lowering is deterministic, so the entry DefIds match the merged unit.
    let package = crate::analyze::analyze_package(
        &config,
        &entry,
        &crate::test_support::test_analysis_seams(),
    )
    .expect("the composition package must analyze");
    let local_roles = package
        .nodes
        .values()
        .find(|node| node.is_entry)
        .map(|node| {
            radix_mir::device_entry_roles_from_hir(&node.analysis.hir, &node.analysis.interner)
        })
        .unwrap_or_default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let facts = imported_entry_facts(lowered);
            let launchable: Vec<_> = facts.iter().filter(|fact| fact.launchable).collect();
            assert_eq!(
                launchable.len(),
                1,
                "the imported add instance registers exactly one launchable Kernel fact: {facts:?}"
            );
            let instance = launchable[0];

            // (d): the launch entry survives its consumer-local caller's
            // composition — present and keyed by the registration's join.
            assert!(
                lowered
                    .validated
                    .program()
                    .functions
                    .iter()
                    .any(|function| function.id == instance.function
                        && function.source == Some(instance.source)),
                "the imported public Kernel instance must stay a program symbol after composition"
            );

            // (a): the consumer-local caller's imported call composed away.
            let probe = function_by_name(lowered, "probe_add");
            assert_eq!(
                call_site_count(probe),
                0,
                "probe_add's imported-instance call must compose at the seam: {probe:#?}"
            );

            // CDC-3 (a): the composed caller is exactly one block — no entry
            // Goto, no synthetic continue block, the caller's original
            // Return terminator — and the merged program classifies Capable
            // at the shared device-safe gate under the merged roles.
            assert_eq!(
                probe.blocks.len(),
                1,
                "the composed consumer caller must be single-block: {probe:#?}"
            );
            assert!(
                matches!(
                    probe.blocks[0].terminator.kind,
                    MirTerminatorKind::Return(_)
                ),
                "the caller's original Return terminator must survive the splice: {probe:#?}"
            );
            let merged = radix_mir::device::merge_imported_entry_roles(
                local_roles.clone(),
                &facts,
                lowered.validated.program(),
            )
            .expect("imported entry facts must merge with the consumer-local roles");
            let mut device = radix_mir::MirDeviceContext::from_hir_roles(merged);
            device.attach_program(lowered.validated.program());
            assert_eq!(
                radix_mir::classify_device_safe_for_target_entry(&lowered.validated, &device),
                radix_types::Lowerability::Capable,
                "the single-block composed program must pass the shared device-safe gate"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "consumer-local kernel package must lower green, got: {:?}",
            errors.iter().filter(|d| d.is_error()).count()
        )
    });
}

/// The package-own analysis entry path (done_when (h) package-own leg): the
/// imported instance comes from a package SIBLING unit (no library, no
/// KPC-WIRE route — the role and launch entry arrive through the consumer
/// collection and the source-rewrite join), and the composition outcome is
/// identical: caller composed, instance retained as a launch symbol.
#[test]
fn sibling_import_composes_consumer_local_kernel_caller() {
    let dir = crate::test_support::test_temp_dir("cdc-sibling-compose");
    let entry = write_sibling_package(dir.path(), SIBLING_MATH_LIB, SIBLING_CONSUMER_KERNEL_CALLER);
    let config = Config::default().with_library_home(dir.join("libhome"));

    // CDC-3: the consumer's local entry roles; a sibling export produces no
    // imported fact, so the merged map is the consumer-local one.
    let package = crate::analyze::analyze_package(
        &config,
        &entry,
        &crate::test_support::test_analysis_seams(),
    )
    .expect("the sibling package must analyze");
    let local_roles = package
        .nodes
        .values()
        .find(|node| node.is_entry)
        .map(|node| {
            radix_mir::device_entry_roles_from_hir(&node.analysis.hir, &node.analysis.interner)
        })
        .unwrap_or_default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            // SGR-U0 superseded the "no library join ⇒ no registration"
            // boundary: a sibling device export is an imported-device route
            // for every consumer unit of the package (package-owned body,
            // composable by construction), so its generic instantiation
            // registers here too — through the source-rewrite join, not the
            // library interface path.
            assert_eq!(
                lowered.imported_device_registrations.len(),
                1,
                "the sibling's public kernel registers exactly one imported instance: {:?}",
                lowered.imported_device_registrations
            );
            assert_eq!(
                lowered.imported_device_registrations[0].route.identity.name, "add",
                "the registered sibling route names the exported verb"
            );

            // The sibling's instantiated public kernel stays a launch symbol:
            // its synthetic source survives in the merged program.
            let instance_sources: Vec<_> = lowered
                .validated
                .program()
                .functions
                .iter()
                .filter_map(|function| function.source)
                .filter(|source| source.0 >= PACKAGE_MIR_SYNTHETIC_DEF_BASE)
                .collect();
            assert!(
                !instance_sources.is_empty(),
                "the sibling's instantiated public kernel must stay a program symbol"
            );

            // Same composition outcome as the library leg.
            let probe = function_by_name(lowered, "probe_add");
            assert_eq!(
                call_site_count(probe),
                0,
                "probe_add's sibling-instance call must compose at the seam: {probe:#?}"
            );

            // CDC-3 (a): the same single-block shape and the same Capable
            // verdict through the package-own analysis leg.
            assert_eq!(
                probe.blocks.len(),
                1,
                "the composed consumer caller must be single-block: {probe:#?}"
            );
            assert!(
                matches!(
                    probe.blocks[0].terminator.kind,
                    MirTerminatorKind::Return(_)
                ),
                "the caller's original Return terminator must survive the splice: {probe:#?}"
            );
            let merged = radix_mir::device::merge_imported_entry_roles(
                local_roles.clone(),
                &imported_entry_facts(lowered),
                lowered.validated.program(),
            )
            .expect("the consumer-local role map must merge with the sibling registration facts");
            let mut device = radix_mir::MirDeviceContext::from_hir_roles(merged);
            device.attach_program(lowered.validated.program());
            assert_eq!(
                radix_mir::classify_device_safe_for_target_entry(&lowered.validated, &device),
                radix_types::Lowerability::Capable,
                "the single-block composed program must pass the shared device-safe gate"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "sibling consumer-local kernel package must lower green, got: {:?}",
            errors.iter().filter(|d| d.is_error()).count()
        )
    });
}

/// done_when (b): a consumer-local private kernel helper's imported call
/// composes at the seam while producing no launch row. Rule 12.7 forbids a
/// host call into a private kernel, so the helper is reached through a public
/// kernel wrapper (`solo_entry`): the imported-instance call and the private
/// helper both splice into the wrapper, the helper keeps no launch ABI (it is
/// not even a surviving program symbol), and only the imported public Kernel
/// registers launchable.
#[test]
fn private_kernel_helper_composes_without_launch_row() {
    let dir = crate::test_support::test_temp_dir("cdc-private-helper");
    let entry = write_compose_package(dir.path(), COMPOSE_MATH_LIB, CONSUMER_PRIVATE_HELPER_CALLER);
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let facts = imported_entry_facts(lowered);
            // The only launchable fact is the imported public Kernel — the
            // private helper produces no launch row on any fact.
            assert_eq!(
                facts.iter().filter(|fact| fact.launchable).count(),
                1,
                "only the imported public Kernel instance is launchable: {facts:?}"
            );

            // The private helper composed into its kernel caller and left no
            // program symbol behind (no launch ABI, no host reference).
            assert!(
                !lowered
                    .validated
                    .program()
                    .functions
                    .iter()
                    .any(|function| {
                        function
                            .name
                            .is_some_and(|symbol| lowered.interner.resolve(symbol) == "solo_helper")
                    }),
                "the private helper must compose away, not survive as a symbol"
            );

            // The imported-instance call (and the helper call) composed away
            // at the seam: no call site survives in the wrapper.
            let wrapper = function_by_name(lowered, "solo_entry");
            assert_eq!(
                call_site_count(wrapper),
                0,
                "the private helper's imported call must compose at the seam: {wrapper:#?}"
            );

            // CDC-3: the composed body is single-block with its original
            // Return terminator — same splice, no launch row.
            assert_eq!(
                wrapper.blocks.len(),
                1,
                "the composed private helper must be single-block: {wrapper:#?}"
            );
            assert!(
                matches!(
                    wrapper.blocks[0].terminator.kind,
                    MirTerminatorKind::Return(_)
                ),
                "the original Return terminator must survive the splice: {wrapper:#?}"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "private-helper package must lower green, got: {:?}",
            errors
                .iter()
                .map(|d| format!("{} issue={:?}", d.message, d.issue()))
                .collect::<Vec<_>>()
        )
    });
}

/// done_when (c): host callers of imported instances are never composed —
/// the host call site survives the seam untouched while the instance still
/// registers launchable (spike3/tier shape).
#[test]
fn host_caller_of_imported_instance_is_never_composed() {
    let dir = crate::test_support::test_temp_dir("cdc-host-caller");
    let entry = write_compose_package(dir.path(), COMPOSE_MATH_LIB, CONSUMER_HOST_CALLER);
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let facts = imported_entry_facts(lowered);
            let launchable: Vec<_> = facts.iter().filter(|fact| fact.launchable).collect();
            assert_eq!(
                launchable.len(),
                1,
                "the imported instance registers: {facts:?}"
            );
            let instance = launchable[0];

            // The host call site still targets the instance: composition
            // never touches the launch boundary.
            let main = function_by_name(lowered, "main");
            let host_call = main.blocks.iter().any(|block| {
                block.statements.iter().any(|statement| {
                    matches!(
                        &statement.kind,
                        MirStatementKind::Call {
                            callee: MirCallee::Function(id),
                            ..
                        } if *id == instance.function
                    )
                })
            });
            assert!(
                host_call,
                "the host caller's instance call must survive composition untouched"
            );

            // The instance body itself is present (never composed away).
            assert!(
                lowered
                    .validated
                    .program()
                    .functions
                    .iter()
                    .any(|function| function.id == instance.function),
                "the imported instance must remain in the merged program"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "host-caller package must lower green, got: {:?}",
            errors.iter().filter(|d| d.is_error()).count()
        )
    });
}

/// done_when (e): a consumer-local caller whose imported chain cycles fails
/// closed at the seam with the same named identity imported-instance callers
/// hit — `nucleum_fragment_cycle` (the self-recursive imported template is
/// the one library shape whose chain reaches the seam; the two-template
/// chain shape collapses shape params at the pre-existing library merge —
/// see the module doc's named boundary).
#[test]
fn consumer_local_cycle_fails_closed_with_named_identity() {
    let dir = crate::test_support::test_temp_dir("cdc-cycle");
    let entry = write_compose_package(dir.path(), CYCLE_MATH_LIB, CONSUMER_CYCLE_CALLER);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let errors = with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |_| (),
    )
    .expect_err("a consumer-local device cycle must fail closed at the seam");

    assert!(
        errors
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("nucleum_fragment_cycle")),
        "expected the named nucleum_fragment_cycle identity, got issues {:?}",
        errors
            .iter()
            .filter_map(|diagnostic| diagnostic.issue())
            .collect::<Vec<_>>()
    );
}

/// done_when (e) budget clause — named boundary: the depth-budget identity
/// (`nucleum_fragment_expansion`, observed/limit args) is unreachable
/// through this seam today because an imported depth CHAIN needs a library
/// template whose body calls another generic template, and that source
/// shape collapses its shape params at the pre-existing library merge
/// (`call argument type mismatch ... expected [N, N]`) before composition
/// runs — the same pre-CDC-1 failure, untouched by the consumer-local
/// union. The identity itself stays pinned with consumer-local inputs at
/// the seam API (`package_instantiate_test.rs`) and with the full
/// observed/limit args in `fragment_composition_test.rs`.
#[test]
#[ignore = "blocked by the pre-existing template-chain merge collapse; see the module doc boundary"]
fn consumer_local_budget_overflow_fails_closed_with_named_identity() {
    let dir = crate::test_support::test_temp_dir("cdc-budget");
    let entry = write_compose_package(dir.path(), &deep_chain_library(24), CONSUMER_BUDGET_CALLER);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let errors = with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |_| (),
    )
    .expect_err("a consumer-local depth overflow must fail closed at the seam");

    assert!(
        errors
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("nucleum_fragment_expansion")),
        "expected the named nucleum_fragment_expansion identity, got issues {:?}",
        errors
            .iter()
            .filter_map(|diagnostic| diagnostic.issue())
            .collect::<Vec<_>>()
    );
}
