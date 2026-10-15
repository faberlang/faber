//! The device fault-delivery markers (`MirIntrinsic::DeviceBlock`) are
//! runner-only (gpu-reset U9-5/U9-6): a program whose unit declares a `⇥`
//! kernel emits on every MIR text target without the markers leaking into the
//! text or refusing the program.

use super::test_support::{compile_metal_text, compile_target_result, compile_target_text_or_panic, compile_wgsl_text};
use crate::codegen::Target;

/// A scalar `⇥ string` kernel and a `do all` caller: the unit declares a `⇥`
/// kernel, so the handled block carries the markers.
const SCALAR: &str = r#"
@ kernel
@ public
fn k(f32 x) → f32 ⇥ string {
    if x ≺ 0.0 {
        throw "negative"
    }
    return x
}

main {
    do all {
        print k(1.0)
    }
    catch err {
        print err
    }
}
"#;

/// The same shape over tensors, for the device text targets.
const TENSOR: &str = r#"
@ kernel
@ public
fn k<size H, size W>(tensor<f32, [H, W]> src, mut tensor<f32, [H, W]> dst) → void ⇥ string {
    for from src at [i, j] thread lane let x {
        if x ≺ 0.0 {
            throw "negative lane"
        }
        dst[[i, j]] ← x * 2.0
    }
}

main {
    mut tensor<f32, [3, 4]> src ← tf32[3, 4] {}
    mut tensor<f32, [3, 4]> dst ← tf32[3, 4] {}
    do all {
        k(src, mut dst)
    }
    catch err {
        print err
    }
}
"#;

/// The English reader surface of `body`.
fn en(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{body}")
}

fn assert_no_markers(text: &str) {
    for marker in ["DeviceBlock", "device_block", "device_deliver", "Deliver {"] {
        assert!(!text.contains(marker), "{marker} leaked: {text}");
    }
}

/// The sexp text is not returned by the shared harness, so the pin is that
/// the program is no longer refused on the marker (before the erase arm the
/// emit failed with `runtime_call DeviceBlock(..)`).
#[test]
fn sexp_no_longer_refuses_the_markers() {
    let result = compile_target_result(Target::MirSexp, "markers.fab", &en(SCALAR));
    assert!(
        result.success(),
        "{}",
        super::test_support::diagnostic_facts_message(&result.diagnostics)
    );
}

#[test]
fn wasm_erases_the_markers_and_keeps_the_handler() {
    let text = compile_target_text_or_panic(Target::MirWasm, "markers.fab", &en(SCALAR));
    assert_no_markers(&text);
    assert!(text.contains("diagnostic_nota"), "the handler survives: {text}");
}

#[test]
fn metal_text_carries_no_marker_for_a_throwing_kernel_unit() {
    let text = compile_metal_text(&en(TENSOR)).unwrap_or_else(|error| panic!("metal text: {error}"));
    assert_no_markers(&text);
    assert!(text.contains("u9_fault"), "the throw site is emitted: {text}");
}

/// The device text targets emit kernels only, so the host-side markers never
/// reach them; a refusal there comes from the kernel itself, never from a
/// marker intrinsic.
#[test]
fn device_text_targets_never_refuse_on_a_marker() {
    for target in [Target::MirLlvm, Target::MirWgsl] {
        let result = compile_target_result(target, "markers.fab", &en(TENSOR));
        let facts = super::test_support::diagnostic_facts_message(&result.diagnostics);
        assert_no_markers(&facts);
    }
    if let Err(message) = compile_wgsl_text(&en(TENSOR)) {
        assert_no_markers(&message);
    }
}
