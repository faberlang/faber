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
    const tensor<f32, []> seed ← empty
    const tensor<f32, [2, 2]> a ← seed.from_flat([1.0, 2.0, 3.0, 4.0], [2, 2])
    const tensor<f32, [2, 2]> b ← seed.from_flat([10.0, 20.0, 30.0, 40.0], [2, 2])
    const tensor<f32, [8, 8]> c ← seed.from_flat([1.0, 2.0, 3.0, 4.0], [8, 8])
    const _ r22 ← devicemath.kadd(a, b)
    const _ again22 ← devicemath.kadd(a, b)
    const _ r88 ← devicemath.kadd(c, c)
    const tensor<f32, [4]> v ← seed.from_flat([1.0, 2.0, 3.0, 4.0], [4])
    const tensor<f32, [4]> w ← seed.from_flat([4.0, 3.0, 2.0, 1.0], [4])
    const _ rh ← devicemath.khelper(v, w)
    print "ok"
}
"#;

fn write_device_package(dir: &std::path::Path) -> std::path::PathBuf {
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
    fs::write(src.join("main.fab"), DEVICE_MAIN).expect("write main.fab");
    src.join("main.fab")
}

fn literal_shape(types: &radix_types::TypeTable, index: radix_types::IndexId) -> Option<i64> {
    match types.get_index(index) {
        radix_module::semantic::IndexExpr::Literal(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

/// done_when (a), (d): one concrete `MirMonomorphizationKey` per requested
/// type/shape binding, with the route's provider identity and v2 device
/// contract carried into the returned package lowering metadata — public
/// Kernel and unmarked private-helper cases both register.
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

            let types = lowered.validated.validation().types;
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
                    .program
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

            // Unmarked `@ kernel` helper: private launch visibility, still an
            // importable export that registers its concrete instance.
            let khelper = registration_of("khelper");
            assert_eq!(
                khelper.route.contract.role,
                radix_module::file_interface::InterfaceDeviceRole::Kernel
            );
            assert_eq!(
                khelper.route.contract.visibility,
                radix_module::file_interface::InterfaceDeviceVisibility::Private
            );
            assert!(khelper.route.contract.is_composable());
            let helper_shapes = khelper
                .key
                .shape_args
                .iter()
                .filter_map(|index| literal_shape(types, *index))
                .collect::<Vec<_>>();
            assert_eq!(helper_shapes, vec![4], "khelper instance must bind [4]");
        },
    )
    .unwrap_or_else(|errors| panic!("device package MIR must link, got {:?}", errors));
}

/// done_when (g): repeated imports and instantiations deduplicate by
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
    .unwrap_or_else(|errors| panic!("device package MIR must link, got {:?}", errors));
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
    const tensor<f32, []> seed ← empty
    const tensor<f32, [4]> v ← seed.from_flat([1.0, 2.0, 3.0, 4.0], [4])
    const _ r ← sigonly.ksig(v)
    print "ok"
}
"#;

/// done_when (e), (f): a signature-only imported device route fails closed
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
