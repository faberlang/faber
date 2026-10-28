//! A kernel that calls an imported function must call a kernel: a host
//! function behind an import is `SEM008 nucleum_host_call` at package link,
//! the same rule single-module analysis applies to a same-module host call.

use super::fs;
use super::{Config, with_interpreted_lowered_package_mir};

const HELPER: &str = r#"+++
locale = "en"
+++
@ public
fn host_mul(tensor<f32, [4]> a, tensor<f32, [4]> b) → tensor<f32, [4]> {
    return a ⊙ b
}

@ kernel
@ public
fn device_mul(tensor<f32, [4]> a, tensor<f32, [4]> b) → tensor<f32, [4]> {
    return a ⊙ b
}
"#;

fn main_calling(callee: &str) -> String {
    format!(
        r#"+++
locale = "en"
+++
import from "./helper" helper

@ kernel
@ public
fn wrap(tensor<f32, [4]> a, tensor<f32, [4]> b) → tensor<f32, [4]> {{
    return helper.{callee}(a, b)
}}

main {{
    let list<f32> d ← [1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [4]> a ← d ↦ tensor<f32, [4]>
    let _ r ← wrap(a, a)
    print "ok"
}}
"#
    )
}

fn lower(name: &str, callee: &str) -> Result<(), Vec<radix_module::diagnostics::Diagnostic>> {
    let dir = crate::test_support::test_temp_dir(name);
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        "[package]\nname = \"kic\"\nversion = \"1.0.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n",
    )
    .expect("write faber.toml");
    fs::write(src.join("helper.fab"), HELPER).expect("write helper");
    fs::write(src.join("main.fab"), main_calling(callee)).expect("write main");
    let config = Config::default().with_library_home(dir.join("libhome"));
    with_interpreted_lowered_package_mir(
        &config,
        &src.join("main.fab"),
        &crate::test_support::test_mir_seams(),
        |_| (),
    )
}

#[test]
fn a_kernel_calling_an_imported_host_function_is_a_named_error() {
    let errors = lower("kernel-import-host", "host_mul").expect_err("host call from a kernel");
    let error = errors
        .iter()
        .find(|error| error.code == Some("SEM008"))
        .unwrap_or_else(|| panic!("SEM008 expected, got {errors:?}"));
    assert!(
        error
            .args
            .iter()
            .any(|arg| arg.name == "callee" && arg.value == "helper.host_mul"),
        "the diagnostic names the qualified callee: {:?}",
        error.args
    );
    assert!(
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "nucleum_host_call"),
        "named issue: {:?}",
        error.args
    );
    assert!(
        error
            .args
            .iter()
            .any(|arg| arg.name == "reason" && arg.value == "not_a_kernel"),
        "the imported contract says the callee is not a kernel: {:?}",
        error.args
    );
}

#[test]
fn a_kernel_calling_an_imported_kernel_links() {
    lower("kernel-import-kernel", "device_mul")
        .unwrap_or_else(|errors| panic!("kernel-to-kernel import must link, got {errors:?}"));
}
