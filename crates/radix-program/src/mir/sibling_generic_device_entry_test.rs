//! SGR-U0: imported generic device entries instantiate at their consumers.
//!
//! The `nn.fab` `linear<size M, K, N>` shape exported by a package SIBLING
//! unit and instantiated by a host consumer cross-module must register its
//! concrete instance as a device function of the compiling package with the
//! use site's ordered size bindings — the same admission the library-import
//! route already carries (KPC-WIRE). A sibling device export is not a
//! library route, but it is an imported route for every consumer unit of the
//! package: its body is package-owned source, so the route is composable by
//! construction.

use super::*;

const SIBLING_NN: &str = r#"+++
locale = "en"
+++

# The nn.fab `linear` shape (gradus/src/nn.fab): one size-generic `@ kernel`
# leaf exported public, consumed cross-module with concrete bindings at the
# use site.
@ kernel
@ public
fn linear<size M, size K, size N>(tensor<f32, [M, K]> x, tensor<f32, [K, N]> w, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return (x · w) + b
}
"#;

const HOST_CONSUMER: &str = r#"+++
locale = "en"
+++

import from "./nn" nn

fn main() → void {
    let _ x ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    let _ w ← [[1.0, 0.0], [0.0, 1.0]] ↦ tensor<f32, [2, 2]>
    let _ b ← [[0.1, 0.1], [0.1, 0.1]] ↦ tensor<f32, [2, 2]>
    let _ r ← nn.linear(x, w, b)
    print "ok"
}
"#;

fn write_sibling_generic_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "sgr0-sibling-generic"
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
    fs::write(src.join("nn.fab"), SIBLING_NN).expect("write sibling module");
    fs::write(src.join("main.fab"), HOST_CONSUMER).expect("write main.fab");
    src.join("main.fab")
}

fn literal_shape(types: &radix_types::TypeTable, index: radix_types::IndexId) -> Option<i64> {
    match types.get_index(index) {
        radix_module::semantic::IndexExpr::Literal(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

/// The SGR-U0 done-when: an imported generic `@ kernel` entry (the
/// `linear<size M, K, N>` shape) consumed cross-module by a host caller
/// checks and instantiates to a device registration whose ordered size
/// bindings are the use site's concrete shapes.
#[test]
fn sibling_generic_kernel_registers_with_ordered_use_site_bindings() {
    let dir = crate::test_support::test_temp_dir("sgr0-sibling-generic");
    let entry = write_sibling_generic_package(dir.path());
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let registration = lowered
                .imported_device_registrations
                .iter()
                .find(|registration| registration.route.identity.name == "linear")
                .unwrap_or_else(|| {
                    panic!(
                        "`linear` must register for the consumer, got {:?}",
                        lowered
                            .imported_device_registrations
                            .iter()
                            .map(|registration| registration.route.identity.name.clone())
                            .collect::<Vec<_>>()
                    )
                });

            // Provider identity: the package product + the sibling module.
            assert!(!registration.route.identity.product.is_empty());
            assert_eq!(
                registration.route.identity.module_path,
                vec!["nn".to_owned()]
            );
            assert_eq!(
                registration.route.contract.role,
                radix_module::file_interface::InterfaceDeviceRole::Kernel
            );
            assert_eq!(
                registration.route.contract.visibility,
                radix_module::file_interface::InterfaceDeviceVisibility::Public
            );
            assert!(registration.route.contract.is_composable());

            // Ordered bindings from the use site: M, K, N = 2, 2, 2 in
            // declaration order.
            let types = lowered.validated.validation().types.as_table();
            let shapes = registration
                .key
                .shape_args
                .iter()
                .filter_map(|index| literal_shape(types, *index))
                .collect::<Vec<_>>();
            assert_eq!(
                shapes,
                vec![2, 2, 2],
                "linear must bind [M, K, N] = [2, 2, 2]"
            );

            // The concrete instance exists in the merged program and joins
            // the merged synthetic template source.
            assert!(
                lowered
                    .validated
                    .program()
                    .functions
                    .iter()
                    .any(|function| function.id == registration.function),
                "the registered linear instance must exist in the merged program"
            );
            assert!(
                lowered
                    .generic_templates
                    .iter()
                    .any(|template| template.source == Some(registration.key.source)),
                "the registration key must join the merged synthetic template source"
            );

            // The registration admits a launchable target entry (KPC-EMIT
            // fact shape): public composable Kernel.
            let facts = imported_entry_facts(lowered);
            assert!(
                facts
                    .iter()
                    .any(|fact| fact.launchable && fact.function == registration.function),
                "the imported linear instance must admit as a launchable entry: {facts:?}"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!("sibling generic device package must lower green, got {errors:?}")
    });
}
