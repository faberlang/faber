//! KPC-WIRE: production package lowering registers imported device instances.
//!
//! A size-generic `@ kernel` exported by a linked library and instantiated by
//! a host consumer must register its concrete instance as a device function
//! of the compiling package — the spike-3 shape, where the Kernel role
//! previously never crossed the library-import boundary. The proofs pin the
//! joined route (identity + v2 contract), the concrete monomorphization key,
//! registration dedup by key, and the fail-closed signature-only boundary.

use super::fs;
#[cfg(test)]
use super::{Config, with_interpreted_lowered_package_mir};

const DEVICE_MATH_LIB: &str = r#"+++
locale = "en"
+++
@ kernel
@ public
fn kadd<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}

@ kernel
fn khelper<size M>(tensor<f32, [M]> a, tensor<f32, [M]> b) → tensor<f32, [M]> {
    return a + b
}

# Same-unit callers keep the generic entries non-orphaned at the library's
# own analysis (ADM001); the consumer below instantiates NEW shapes across
# the import boundary, which is the spike-3 shape this unit wires.
@ public
fn step_2x2(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {
    return kadd(a, b)
}

@ kernel
@ public
fn helper_4(tensor<f32, [4]> a, tensor<f32, [4]> b) → tensor<f32, [4]> {
    return khelper(a, b)
}
"#;

const DEVICE_MAIN: &str = r#"+++
locale = "en"
+++
import from "optlib:devicemath" devicemath

main {
    let tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    let tensor<f32, [2, 2]> b ← [[10.0, 20.0], [30.0, 40.0]] ↦ tensor<f32, [2, 2]>
    let list<f32> flat_c ← [1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [8, 8]> c ← flat_c ↦ tensor<f32, [8, 8]>
    let _ r22 ← devicemath.kadd(a, b)
    let _ again22 ← devicemath.kadd(a, b)
    let _ r88 ← devicemath.kadd(c, c)
    print "ok"
}
"#;

/// DECISIONS D5.1: the unmarked `khelper` kernel is module-private, so a
/// cross-package consumer that names it is rejected at the import seam.
const DEVICE_MAIN_PRIVATE_HELPER: &str = r#"+++
locale = "en"
+++
import from "optlib:devicemath" devicemath

main {
    let tensor<f32, [4]> v ← [1.0, 2.0, 3.0, 4.0] ↦ tensor<f32, [4]>
    let tensor<f32, [4]> w ← [4.0, 3.0, 2.0, 1.0] ↦ tensor<f32, [4]>
    let _ rh ← devicemath.khelper(v, w)
    print "ok"
}
"#;

fn write_device_package(dir: &std::path::Path) -> std::path::PathBuf {
    write_device_package_with_main(dir, DEVICE_MAIN)
}

fn write_device_package_with_main(dir: &std::path::Path, main: &str) -> std::path::PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library = libhome.join("optlib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "generic-device-link"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
optlib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"optlib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library.join("src/devicemath.fab"), DEVICE_MATH_LIB).expect("write library module");
    fs::write(src.join("main.fab"), main).expect("write main.fab");
    src.join("main.fab")
}

fn literal_shape(types: &radix_types::TypeTable, index: radix_types::IndexId) -> Option<i64> {
    match types.get_index(index) {
        radix_module::semantic::IndexExpr::Literal(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

/// `done_when` (a), (d): one concrete `MirMonomorphizationKey` per requested
/// type/shape binding, with the route's provider identity and v2 device
/// contract carried into the returned package lowering metadata. Under
/// DECISIONS D5.1 only the `@ public` Kernel crosses the package boundary:
/// the unmarked `khelper` is module-private and never becomes an imported
/// device route (its private launch tier is proven through a same-package
/// `@ internal` sibling in
/// `package_mir_internal_sibling_device_helper_registers_private`).
#[test]
fn package_mir_registers_imported_device_instances() {
    let dir = crate::test_support::test_temp_dir("generic-device-link");
    let entry = write_device_package(dir.as_ref());
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert!(
                !lowered.imported_device_registrations.is_empty(),
                "imported generic device instances must register for the compiling package"
            );

            let types = lowered.validated.validation().types.as_table();
            let registration_of = |verb: &str| {
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
            };

            // Public `@ kernel` route: provider identity joined at the link.
            let kadd = registration_of("kadd");
            assert_eq!(kadd.route.identity.product, "optlib");
            assert_eq!(
                kadd.route.identity.module_path,
                vec!["devicemath".to_owned()]
            );
            assert_eq!(
                kadd.route.contract.role,
                radix_module::file_interface::InterfaceDeviceRole::Kernel
            );
            assert!(kadd.route.contract.is_composable());

            // (a) the key binds the requested shape witnesses concretely.
            let shapes = kadd
                .key
                .shape_args
                .iter()
                .filter_map(|index| literal_shape(types, *index))
                .collect::<Vec<_>>();
            assert_eq!(
                shapes,
                vec![2, 2],
                "kadd instance must bind [2, 2], got {shapes:?}"
            );

            // The registered instance exists in the merged program and is
            // keyed by the same source as the merged synthetic template.
            assert!(
                lowered
                    .validated
                    .program()
                    .functions
                    .iter()
                    .any(|function| function.id == kadd.function),
                "registered instance f{} must exist in the merged program",
                kadd.function.0
            );
            assert!(
                lowered
                    .generic_templates
                    .iter()
                    .any(|template| template.source == Some(kadd.key.source)),
                "registration key must join the merged synthetic template source"
            );

            // Unmarked `@ kernel` helper: module-private under D5.1, so it is
            // never an importable device route of the library.
            assert!(
                lowered
                    .imported_device_registrations
                    .iter()
                    .all(|registration| registration.route.identity.name != "khelper"),
                "the module-private khelper must not register as an imported route"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("device package MIR must link, got {errors:?}"));
}

/// `done_when` (g): repeated imports and instantiations deduplicate by
/// `MirMonomorphizationKey` — two `kadd` calls at [2, 2] and one at [8, 8]
/// leave exactly two kadd registrations, never one per call site.
#[test]
fn package_mir_imported_device_registrations_deduplicate_by_key() {
    let dir = crate::test_support::test_temp_dir("generic-device-link-dedup");
    let entry = write_device_package(dir.as_ref());
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let mut kadd_keys = lowered
                .imported_device_registrations
                .iter()
                .filter(|registration| registration.route.identity.name == "kadd")
                .map(|registration| registration.key.clone())
                .collect::<Vec<_>>();
            kadd_keys.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
            kadd_keys.dedup();
            let raw_count = lowered
                .imported_device_registrations
                .iter()
                .filter(|registration| registration.route.identity.name == "kadd")
                .count();
            assert_eq!(
                raw_count,
                kadd_keys.len(),
                "one registration per monomorphization key, got {raw_count} for {} keys",
                kadd_keys.len()
            );
            assert_eq!(
                kadd_keys.len(),
                2,
                "[2, 2] twice and [8, 8] once must leave two distinct kadd keys"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("device package MIR must link, got {errors:?}"));
}

const SIGNATURE_ONLY_LIB: &str = r#"+++
locale = "en"
+++
@ kernel
@ public
fn ksig<size M>(tensor<f32, [M]> a) → tensor<f32, [M]>
"#;

const SIGNATURE_ONLY_MAIN: &str = r#"+++
locale = "en"
+++
import from "optlib:sigonly" sigonly

main {
    let tensor<f32, [4]> v ← [1.0, 2.0, 3.0, 4.0] ↦ tensor<f32, [4]>
    let _ r ← sigonly.ksig(v)
    print "ok"
}
"#;

/// `done_when` (e), (f): a signature-only imported device route fails closed
/// with the named `imported_device_body_missing` reason — the role bit alone
/// never validates the call, and no provider fallback or backend helper
/// symbol makes the unresolved device call valid.
#[test]
fn package_mir_signature_only_imported_device_fails_closed() {
    let dir = crate::test_support::test_temp_dir("generic-device-signature-only");
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library = libhome.join("optlib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "generic-device-signature-only"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
optlib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"optlib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library.join("src/sigonly.fab"), SIGNATURE_ONLY_LIB).expect("write library module");
    fs::write(src.join("main.fab"), SIGNATURE_ONLY_MAIN).expect("write main.fab");
    let entry = src.join("main.fab");
    let config = Config::default().with_library_home(dir.join("libhome"));

    let errors = with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            panic!(
                "signature-only imported device must fail closed, got {} registrations",
                lowered.imported_device_registrations.len()
            )
        },
    )
    .expect_err("signature-only route must fail the package link");
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

/// DECISIONS D5.1 converse of the old "unmarked helper stays importable"
/// shape: a cross-package consumer naming the unmarked (module-private)
/// `khelper` kernel is rejected at the library import seam with the named
/// `namespace_missing_export` issue.
#[test]
fn package_mir_module_private_device_helper_is_not_importable_cross_package() {
    let dir = crate::test_support::test_temp_dir("generic-device-private-helper");
    write_device_package_with_main(dir.as_ref(), DEVICE_MAIN_PRIVATE_HELPER);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let diagnostics = crate::analyze::check_package(
        &config,
        &dir.join("faber.toml"),
        &crate::test_support::test_analysis_seams(),
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("namespace_missing_export")),
        "the module-private helper must not be importable cross-package, got {:?}",
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
            .collect::<Vec<_>>()
    );
}

const INTERNAL_SIBLING_HELPERS: &str = r#"+++
locale = "en"
+++
@ kernel
@ internal
fn khelper<size M>(tensor<f32, [M]> a, tensor<f32, [M]> b) → tensor<f32, [M]> {
    return a + b
}

# Same-unit caller keeps the generic entry non-orphaned (ADM001), mirroring
# the library fixture's `helper_4`. It is a kernel: rule 12.7 forbids a host
# call into a private kernel, so the private helper is only ever composed by
# another kernel.
@ kernel
@ internal
fn helper_4(tensor<f32, [4]> a, tensor<f32, [4]> b) → tensor<f32, [4]> {
    return khelper(a, b)
}
"#;

const INTERNAL_SIBLING_MAIN: &str = r#"+++
locale = "en"
+++
import from "./helpers" helpers

# The sibling's `@ internal` kernel is private (rule 12.7), so it is imported
# by a kernel; the host reaches it only through this public wrapper.
@ kernel
@ public
fn probe_helper(tensor<f32, [4]> a, tensor<f32, [4]> b) → tensor<f32, [4]> {
    return helpers.khelper(a, b)
}

main {
    let tensor<f32, [4]> v ← [1.0, 2.0, 3.0, 4.0] ↦ tensor<f32, [4]>
    let tensor<f32, [4]> w ← [4.0, 3.0, 2.0, 1.0] ↦ tensor<f32, [4]>
    let _ rh ← probe_helper(v, w)
    print "ok"
}
"#;

/// The private launch tier that the unmarked cross-package helper used to
/// carry, expressed through what D5.1 still admits: an `@ internal` kernel
/// imported by a same-package sibling registers its concrete instance with
/// `Private` launch visibility (only `@ public` is a Public launch fact).
#[test]
fn package_mir_internal_sibling_device_helper_registers_private() {
    let dir = crate::test_support::test_temp_dir("generic-device-internal-sibling");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "generic-device-internal-sibling"
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
    fs::write(src.join("helpers.fab"), INTERNAL_SIBLING_HELPERS).expect("write helpers.fab");
    fs::write(src.join("main.fab"), INTERNAL_SIBLING_MAIN).expect("write main.fab");
    let entry = src.join("main.fab");
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let khelper = lowered
                .imported_device_registrations
                .iter()
                .find(|registration| registration.route.identity.name == "khelper")
                .unwrap_or_else(|| {
                    panic!(
                        "the internal sibling helper must register; got {:?}",
                        lowered
                            .imported_device_registrations
                            .iter()
                            .map(|registration| registration.route.identity.name.clone())
                            .collect::<Vec<_>>()
                    )
                });
            assert_eq!(
                khelper.route.contract.role,
                radix_module::file_interface::InterfaceDeviceRole::Kernel
            );
            assert_eq!(
                khelper.route.contract.visibility,
                radix_module::file_interface::InterfaceDeviceVisibility::Private
            );
            assert!(khelper.route.contract.is_composable());
            let types = lowered.validated.validation().types.as_table();
            let helper_shapes = khelper
                .key
                .shape_args
                .iter()
                .filter_map(|index| literal_shape(types, *index))
                .collect::<Vec<_>>();
            assert_eq!(helper_shapes, vec![4], "khelper instance must bind [4]");
        },
    )
    .unwrap_or_else(|errors| panic!("internal sibling device helper must link, got {errors:?}"));
}
