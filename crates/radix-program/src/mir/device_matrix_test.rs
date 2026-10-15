//! DFV2-6 compiler/package contract matrix (`device-fragments-v2` DFV2-6).
//!
//! One integrated sweep over the production package path for the landed v2
//! family (DFV2-1..5, PKG-ADM, PKG-REP, KPC-SEM, KPC-WIRE, KPC-EMIT): a
//! package-owned size-generic kernel twin is admitted rather than orphaned,
//! its twin signature is representable, the consumer's concrete call carries
//! real type/shape witnesses, and package lowering records the concrete
//! `MirMonomorphizationKey` joined to the merged synthetic template with an
//! `ImportedDeviceRegistration` whose visibility tier is the launch fact.
//! The fail-closed family pins that no internal-tier template,
//! signature-only body, unknown callee, or fragment-role export is accepted
//! as a hidden fallback.
//!
//! This unit owns the integrated identity only. The focused family proofs
//! stay with their units: PKG-ADM in
//! `radix-module/src/driver/device_orphan_admission_test.rs` and
//! `radix-program/src/analyze_test.rs`, PKG-REP in the two
//! `file_interface_test.rs` files, eval-once/transitive/cycle/budget
//! composition in `radix-module/src/mir/fragment_composition_test.rs`, and
//! package round-trip + cache identity in `radix-module/src/hir/package_test.rs`.

use super::*;

/// The gradus `math.fab` twin shape: an exported public size-generic
/// `@ kernel` template with ordered symbolic shape witnesses and **no
/// same-unit caller** — a package API, not an orphan. (DECISIONS D5.1: an
/// unmarked helper twin is module-private and no longer importable across
/// the package boundary; see [`UNMARKED_TWIN_HELPER`].)
const TWIN_LIB: &str = r#"+++
locale = "en"
+++
@ kernel
@ public
fn twin_add<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}
"#;

/// The helper twin the pre-D5.1 matrix imported with private launch
/// visibility: unmarked, so under DECISIONS D5.1 it is module-private. A
/// same-unit caller keeps it non-orphaned (ADM001) at the library's own
/// analysis, so the proof isolates the consumer's import seam.
const UNMARKED_TWIN_HELPER: &str = r#"
@ kernel
fn twin_hidden<size M>(tensor<f32, [M]> a, tensor<f32, [M]> b) → tensor<f32, [M]> {
    return a + b
}

@ kernel
@ public
fn hidden_4(tensor<f32, [4]> a, tensor<f32, [4]> b) → tensor<f32, [4]> {
    return twin_hidden(a, b)
}
"#;

/// The consumer instantiates NEW shapes across the import boundary with
/// concrete type/shape witnesses (the KPC-SEM contract exercised through the
/// full package path, not the semantic unit's focused harness).
const TWIN_MAIN: &str = r#"+++
locale = "en"
+++
import from "twinslib:twins" twins

main {
    let tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    let tensor<f32, [2, 2]> b ← [[10.0, 20.0], [30.0, 40.0]] ↦ tensor<f32, [2, 2]>
    let tensor<f32, [4]> v ← [1.0, 2.0, 3.0, 4.0] ↦ tensor<f32, [4]>
    let _ r ← twins.twin_add(a, b)
    print "ok"
}
"#;

/// Write the two-package fixture (consumer bin + `twinslib` source library)
/// and return the consumer entry path.
fn write_twins_package(dir: &Path, library: &str, main: &str) -> PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library_root = libhome.join("twinslib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library_root.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "device-matrix"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
twinslib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library_root.display();
    let lock = format!(
        "[[package]]\nname = \"twinslib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library_root.join("src/twins.fab"), library).expect("write library module");
    fs::write(src.join("main.fab"), main).expect("write main.fab");
    src.join("main.fab")
}

fn matrix_dir(label: &str) -> crate::test_support::TestDir {
    crate::test_support::test_temp_dir(label)
}

fn twins_config(dir: &Path) -> Config {
    Config::default().with_library_home(dir.join("libhome"))
}

fn literal_shape(types: &radix_types::TypeTable, index: radix_types::IndexId) -> Option<i64> {
    match types.get_index(index) {
        radix_module::semantic::IndexExpr::Literal(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

fn registration_of<'a>(
    lowered: &'a radix_mir_lower::lower::LoweredMirUnit,
    verb: &str,
) -> &'a radix_mir_lower::lower::ImportedDeviceRegistration {
    lowered
        .imported_device_registrations
        .iter()
        .find(|registration| registration.route.identity.name == verb)
        .unwrap_or_else(|| {
            panic!(
                "`{verb}` must register; got {:?}",
                lowered
                    .imported_device_registrations
                    .iter()
                    .map(|registration| registration.route.identity.name.clone())
                    .collect::<Vec<_>>()
            )
        })
}

/// Package-aware generic admission + twin-signature representability, the
/// integrated PKG-ADM/PKG-REP identity: the exported public template with no
/// same-unit caller produces no `ADM001` orphan and its symbolic twin
/// signature produces no `PKG001` unrepresentable error through the full
/// package graph — and the fixture package checks clean overall.
#[test]
fn exported_generic_twin_template_is_admitted_and_representable_in_package_mode() {
    let dir = matrix_dir("device-matrix-admission");
    write_twins_package(dir.as_ref(), TWIN_LIB, TWIN_MAIN);
    let config = twins_config(dir.as_ref());

    let diagnostics = crate::analyze::check_package(
        &config,
        &dir.join("faber.toml"),
        &crate::test_support::test_analysis_seams(),
    );
    let family: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.issue(),
                Some("nucleum_orphaned_annotation") | Some("interface_export_unrepresentable")
            )
        })
        .collect();
    assert!(
        family.is_empty(),
        "the exported twin template must be package-admitted and its twin signature representable: {family:?}"
    );
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .collect();
    assert!(
        errors.is_empty(),
        "the matrix fixture must check clean: {errors:?}"
    );
}

/// Concrete key recording + route joining + registration + entry visibility +
/// body presence (KPC-WIRE absorbed): the consumer's `[2, 2]` call records
/// one `MirMonomorphizationKey` bound to the requested shape witnesses, the
/// registration's route carries the provider identity and v2 contract, the
/// key joins the merged synthetic template source, and the visibility tier
/// records the public entry fact. (Pre-D5.1 this also registered an unmarked
/// cross-package helper with private launch visibility; that helper is now
/// module-private and rejected at import — see
/// `unmarked_twin_helper_is_module_private_and_not_importable` — and the
/// private launch tier is proven through a same-package `@ internal` sibling
/// in `generic_device_link_test.rs`.)
#[test]
fn imported_twin_instance_records_key_route_registration_and_visibility() {
    let dir = matrix_dir("device-matrix-registration");
    let entry = write_twins_package(dir.as_ref(), TWIN_LIB, TWIN_MAIN);
    let config = twins_config(dir.as_ref());

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let twin_add = registration_of(lowered, "twin_add");
            assert_eq!(
                twin_add.route.identity.product, "twinslib",
                "provider identity must join at the link"
            );
            assert_eq!(
                twin_add.route.identity.module_path,
                vec!["twins".to_owned()]
            );
            assert_eq!(
                twin_add.route.contract.role,
                radix_module::file_interface::InterfaceDeviceRole::Kernel
            );
            assert_eq!(
                twin_add.route.contract.visibility,
                radix_module::file_interface::InterfaceDeviceVisibility::Public
            );
            assert!(
                twin_add.route.contract.is_composable(),
                "the source body channel must back composition"
            );
            let shapes = twin_add
                .key
                .shape_args
                .iter()
                .filter_map(|index| {
                    literal_shape(lowered.validated.validation().types.as_table(), *index)
                })
                .collect::<Vec<_>>();
            assert_eq!(
                shapes,
                vec![2, 2],
                "twin_add must bind [2, 2], got {shapes:?}"
            );
            assert!(
                lowered
                    .validated
                    .program()
                    .functions
                    .iter()
                    .any(|function| function.id == twin_add.function),
                "the registered instance must exist in the merged program"
            );
            let metadata = lowered
                .device_function_metadata
                .get(&twin_add.function)
                .expect("the registered public instance must carry source metadata");
            assert_eq!(metadata.role, radix_mir::MirDeviceRole::Kernel);
            assert_eq!(
                metadata.shader_stage,
                radix_mir::MirKernelShaderStage::Compute
            );
            assert_eq!(
                metadata.visibility,
                radix_mir::MirFunctionVisibility::Public
            );
            assert_eq!(metadata.origin, radix_mir::MirFunctionOrigin::Source);
            assert!(
                lowered
                    .generic_templates
                    .iter()
                    .any(|template| template.source == Some(twin_add.key.source)),
                "the registration key must join the merged synthetic template source"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("matrix package MIR must link, got {:?}", errors));
}

/// DECISIONS D5.1 converse of the pre-D5.1 "unmarked helper twin stays
/// importable" fact: an unmarked size-generic helper twin is module-private,
/// so the consumer's cross-package call is rejected at the library import
/// seam with `namespace_missing_export` — it never reaches the link as a
/// private-visibility route.
#[test]
fn unmarked_twin_helper_is_module_private_and_not_importable() {
    let library = format!("{TWIN_LIB}{UNMARKED_TWIN_HELPER}");
    let main = TWIN_MAIN.replacen(
        "    print \"ok\"",
        "    let tensor<f32, [4]> w ← [4.0, 3.0, 2.0, 1.0] ↦ tensor<f32, [4]>\n    let _ h ← twins.twin_hidden(v, w)\n    print \"ok\"",
        1,
    );
    let dir = matrix_dir("device-matrix-unmarked-helper");
    write_twins_package(dir.as_ref(), &library, &main);
    let config = twins_config(dir.as_ref());

    let diagnostics = crate::analyze::check_package(
        &config,
        &dir.join("faber.toml"),
        &crate::test_support::test_analysis_seams(),
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("namespace_missing_export")),
        "the unmarked helper twin must not be importable cross-package, got {:?}",
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
            .collect::<Vec<_>>()
    );
}

/// Unadmitted-generic fail-closed (integrated PKG-ADM converse): an
/// `@ internal`-tier size-generic kernel template is not a package admission
/// fact — the orphan diagnostic still names the library unit through the
/// consumer's package analysis. Tier-based demand admission covers publica
/// exports only; wanting the template from another package admits nothing.
#[test]
fn internal_tier_twin_template_is_not_admitted_and_stays_orphaned() {
    let library = TWIN_LIB.replacen("@ public", "@ internal", 1);
    let dir = matrix_dir("device-matrix-internal-tier");
    write_twins_package(dir.as_ref(), &library, TWIN_MAIN);
    let config = twins_config(dir.as_ref());

    let diagnostics = crate::analyze::check_package(
        &config,
        &dir.join("faber.toml"),
        &crate::test_support::test_analysis_seams(),
    );
    let orphan = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.issue() == Some("nucleum_orphaned_annotation"))
        .expect("the internal-tier template must stay an ADM001 orphan");
    assert!(
        orphan.file.ends_with("twinslib/src/twins.fab"),
        "the orphan must be reported on the library unit, got {}",
        orphan.file
    );
}

/// Signature-only imported body fail-closed (absorbed KPC-WIRE negative): the
/// role bit and representable twin signature never validate a bodyless
/// device export — the package link fails with the named
/// `imported_device_body_missing` reason instead of a hidden signature-only
/// fallback.
#[test]
fn signature_only_twin_body_fails_closed_at_the_package_link() {
    let library = TWIN_LIB.replacen(
        "fn twin_add<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {\n    return a + b\n}",
        "fn twin_add<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]>",
        1,
    );
    let dir = matrix_dir("device-matrix-signature-only");
    let entry = write_twins_package(dir.as_ref(), &library, TWIN_MAIN);
    let config = twins_config(dir.as_ref());

    let errors = with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            panic!(
                "signature-only twin must fail closed, got {} registrations",
                lowered.imported_device_registrations.len()
            )
        },
    )
    .expect_err("the signature-only twin must fail the package link");
    assert!(
        errors
            .iter()
            .any(|error| error.issue() == Some("imported_device_body_missing")),
        "named fail-closed issue `imported_device_body_missing` required, got {:?}",
        errors
            .iter()
            .map(|error| (error.code, error.issue()))
            .collect::<Vec<_>>()
    );
}

/// Unknown-callee fail-closed: a consumer call with no corresponding export
/// stays an error through the package path — the interface namespace never
/// silently matches an unresolved verb.
#[test]
fn unknown_imported_callee_stays_unresolved() {
    let main = TWIN_MAIN.replacen("twins.twin_add(a, b)", "twins.twin_absent(a, b)", 1);
    let dir = matrix_dir("device-matrix-unknown-callee");
    write_twins_package(dir.as_ref(), TWIN_LIB, &main);
    let config = twins_config(dir.as_ref());

    let diagnostics = crate::analyze::check_package(
        &config,
        &dir.join("faber.toml"),
        &crate::test_support::test_analysis_seams(),
    );
    let unresolved: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .collect();
    assert!(
        !unresolved.is_empty(),
        "an unknown imported callee must stay an error, got {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("namespace_missing_export")),
        "the unresolved callee must fail with the named `namespace_missing_export` issue, got {:?}",
        unresolved
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
            .collect::<Vec<_>>()
    );
}

/// Fragment-role no-hidden-fallback: an exported `@ kernel fragment`
/// composition helper keeps its Fragment device role through the package
/// path — it is never silently reclassified as a compute Kernel entry.
#[test]
fn fragment_role_twin_registers_as_fragment_never_kernel() {
    let library = format!(
        "{TWIN_LIB}\n@ kernel fragment\n@ public\nfn twin_frag<size M>(tensor<f32, [M]> a) → tensor<f32, [M]> {{\n    return a\n}}\n"
    );
    let main = TWIN_MAIN.replacen(
        "    print \"ok\"",
        "    let _ f ← twins.twin_frag(v)\n    print \"ok\"",
        1,
    );
    let dir = matrix_dir("device-matrix-fragment-role");
    let entry = write_twins_package(dir.as_ref(), &library, &main);
    let config = twins_config(dir.as_ref());

    let result = with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let frag = registration_of(lowered, "twin_frag");
            assert_eq!(
                frag.route.contract.role,
                radix_module::file_interface::InterfaceDeviceRole::Fragment,
                "a fragment-role export must keep the Fragment role at registration"
            );
            assert_ne!(
                frag.route.contract.role,
                radix_module::file_interface::InterfaceDeviceRole::Kernel,
                "a fragment-role export must never register as a Kernel entry"
            );
        },
    );
    match result {
        Ok(()) => {}
        Err(errors) => panic!(
            "fragment composition helper must stay composable, got {:?}",
            errors
        ),
    }
}
