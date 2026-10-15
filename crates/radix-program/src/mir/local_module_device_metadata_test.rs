//! A package-local module must not strip the device rows of imported generic
//! kernel instances.
//!
//! With a sibling module the entry's calls into a library generic kernel
//! instantiate during the sibling append, before the library's own device
//! rows have joined the merged map. The instance rows are therefore completed
//! by the later instantiation passes; an instance that never gets its row is
//! not a device function and `faber run --device metal` fails with
//! `E_DEVICE_NONE_ADMITTED` and an empty reason list.

use super::*;

const TWIN_LIB: &str = r#"+++
locale = "en"
+++
@ kernel
@ public
fn twin_add<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}
"#;

const LOCAL_MODULE: &str = r#"+++
locale = "en"
+++
@ public
fn one() → int {
    return 1
}
"#;

const MAIN: &str = r#"+++
locale = "en"
+++
import from "twinslib:twins" twins
import from "./dummy" dummy

main {
    let tensor<f32, [2, 2]> a ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    let tensor<f32, [2, 2]> b ← [[10.0, 20.0], [30.0, 40.0]] ↦ tensor<f32, [2, 2]>
    let r ← twins.twin_add(a, b)
    print dummy.one()
}
"#;

fn write_package(dir: &Path, with_local_module: bool) -> PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library_root = libhome.join("twinslib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library_root.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "local-module-device"
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
    fs::write(library_root.join("src/twins.fab"), TWIN_LIB).expect("write library module");
    let main = if with_local_module {
        MAIN.to_owned()
    } else {
        MAIN.replace("import from \"./dummy\" dummy\n", "")
            .replace("print dummy.one()", "print 1")
    };
    fs::write(src.join("main.fab"), main).expect("write main.fab");
    if with_local_module {
        fs::write(src.join("dummy.fab"), LOCAL_MODULE).expect("write local module");
    }
    src.join("main.fab")
}

/// Every concrete `twin_add` instance in the merged program carries a device
/// metadata row, with and without a package-local module.
fn twin_instances_carry_device_rows(with_local_module: bool) {
    let dir = crate::test_support::test_temp_dir("local-module-device");
    let entry = write_package(dir.as_ref(), with_local_module);
    let config = Config::default().with_library_home(dir.join("libhome"));

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let registration = lowered
                .imported_device_registrations
                .iter()
                .find(|registration| registration.route.identity.name == "twin_add")
                .expect("twin_add must register for the consumer");
            let metadata = lowered
                .device_function_metadata
                .get(&registration.function)
                .unwrap_or_else(|| {
                    panic!(
                        "the registered twin_add instance {:?} must carry a device row; rows: {:?}",
                        registration.function,
                        lowered.device_function_metadata.keys().collect::<Vec<_>>()
                    )
                });
            assert_eq!(metadata.role, radix_mir::MirDeviceRole::Kernel);
            assert_eq!(
                metadata.shader_stage,
                radix_mir::MirKernelShaderStage::Compute
            );
        },
    )
    .unwrap_or_else(|errors| panic!("package MIR must link, got {errors:?}"));
}

#[test]
fn imported_generic_kernel_instance_carries_device_row_without_local_module() {
    twin_instances_carry_device_rows(false);
}

#[test]
fn imported_generic_kernel_instance_carries_device_row_with_local_module() {
    twin_instances_carry_device_rows(true);
}
