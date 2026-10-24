//! A module-private generic kernel helper declared before the public kernel
//! that calls it must not drop the module's device rows: declaration order is
//! not meaning.

use super::*;

const HELPER_FIRST: &str = r#"+++
locale = "en"
+++
@ kernel
fn at<size H, size W>(tensor<f32, [H, W]> src, i32 x, i32 y) → f32 {
    return src[[y, x]]
}

@ kernel
@ public
fn swap<size H, size W>(tensor<f32, [H, W]> src, mut tensor<f32, [H, W]> dst) → void {
    for from src at [i, j] thread lane let v {
        dst[[i, j]] ← at(src, j, i)
    }
}
"#;

const MAIN: &str = r#"+++
locale = "en"
+++
import from "./k" k

main {
    let tensor<f32, [2, 2]> src ← [[1.0, 2.0], [3.0, 4.0]] ↦ tensor<f32, [2, 2]>
    mut tensor<f32, [2, 2]> dst ← tf32[2, 2] {}
    k.swap(src, mut dst)
    print dst[[0, 1]]
}
"#;

fn write_package(dir: &Path, module: &str) -> PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        "[package]\nname = \"helper-order\"\nversion = \"1.0.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n",
    )
    .expect("write faber.toml");
    fs::write(src.join("k.fab"), module).expect("write k.fab");
    fs::write(src.join("main.fab"), MAIN).expect("write main.fab");
    src.join("main.fab")
}

fn swap_carries_device_row(module: &str) {
    let dir = crate::test_support::test_temp_dir("helper-order-device");
    let entry = write_package(dir.as_ref(), module);
    let config = Config::default().with_library_home(dir.join("libhome"));
    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let names = |id: radix_mir::MirFunctionId| {
                lowered
                    .validated
                    .program()
                    .functions
                    .iter()
                    .find(|function| function.id == id)
                    .map(|function| format!("{id:?}={:?}", function.name))
                    .unwrap_or_else(|| format!("{id:?}=?"))
            };
            let registration = lowered
                .imported_device_registrations
                .iter()
                .find(|registration| registration.route.identity.name == "swap")
                .unwrap_or_else(|| {
                    panic!(
                        "swap must register; got {:?}",
                        lowered
                            .imported_device_registrations
                            .iter()
                            .map(|r| (r.route.identity.name.clone(), names(r.function)))
                            .collect::<Vec<_>>()
                    )
                });
            let row = lowered
                .device_function_metadata
                .get(&registration.function)
                .unwrap_or_else(|| {
                    panic!(
                        "swap instance {} must carry a device row; rows: {:?}",
                        names(registration.function),
                        lowered
                            .device_function_metadata
                            .keys()
                            .map(|id| names(*id))
                            .collect::<Vec<_>>()
                    )
                });
            assert_eq!(row.identity.parent, registration.function);
            assert_eq!(
                row.visibility,
                radix_mir::MirFunctionVisibility::Public,
                "the public kernel instance must keep its own (public) row, not a helper's"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("package must lower green, got {errors:?}"));
}

#[test]
fn private_helper_declared_first_keeps_kernel_device_row() {
    swap_carries_device_row(HELPER_FIRST);
}

#[test]
fn private_helper_declared_last_keeps_kernel_device_row() {
    let (header, rest) = HELPER_FIRST.split_at(HELPER_FIRST.find("@ kernel").expect("kernel"));
    let split = rest.find("\n@ kernel\n@ public").expect("public kernel");
    let (helper, public) = rest.split_at(split);
    swap_carries_device_row(&format!("{header}{}\n\n{helper}\n", public.trim_start()));
}
