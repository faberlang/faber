//! Shift views inside a device kernel: `src.shift(s).edge<P>()` read at the
//! lane's own coordinate lowers to a read of `src` at the shifted coordinate,
//! with no view storage, and an edge policy the device cannot express is
//! refused by name.

use super::test_support::compile_metal_text;

fn kernel(body: &str) -> String {
    format!(
        r#"+++
locale = "en"
+++
@ kernel
@ public
fn k<size H, size W>(tensor<f32, [H, W]> src, mut tensor<f32, [H, W]> dst) → void {{
    for from src at [i, j] thread lane let v {{
        {body}
    }}
}}

main {{
    let tensor<f32, [3, 4]> src ← tf32[3, 4] {{}}
    mut tensor<f32, [3, 4]> dst ← tf32[3, 4] {{}}
    k(src, mut dst)
}}
"#
    )
}

fn kernel_body(text: &str) -> &str {
    let start = text
        .find("kernel void")
        .unwrap_or_else(|| panic!("no kernel entry: {text}"));
    &text[start..]
}

#[test]
fn a_clamped_shift_view_reads_its_base_at_the_shifted_coordinate() {
    let text = compile_metal_text(&kernel(
        "let tensor<f32, [H, W]> e ← src.shift([-1, 2]).edge<clamp>()\n        dst[[i, j]] ← v + e[[i, j]]",
    ))
    .unwrap_or_else(|error| panic!("metal text: {error}"));
    let body = kernel_body(&text);
    // A positive offset clamps at the last index, a negative one at zero.
    assert!(body.contains("min("), "{body}");
    assert!(body.contains("max("), "{body}");
    assert!(!body.contains("shift") && !body.contains("limes"), "{body}");
}

#[test]
fn iterating_a_wrapped_shift_view_reads_its_base() {
    let text = compile_metal_text(&kernel("dst[[i, j]] ← v").replace(
        "for from src at [i, j] thread lane let v",
        "for from src.shift([0, -1]).edge<wrap>() at [i, j] thread lane let v",
    ))
    .unwrap_or_else(|error| panic!("metal text: {error}"));
    let body = kernel_body(&text);
    assert!(body.contains('%'), "{body}");
    assert!(!body.contains("shift") && !body.contains("limes"), "{body}");
}

#[test]
fn a_reflected_shift_view_is_refused_by_name() {
    let error = compile_metal_text(&kernel(
        "let tensor<f32, [H, W]> e ← src.shift([0, 1]).edge<reflect>()\n        dst[[i, j]] ← e[[i, j]]",
    ))
    .expect_err("reflect is not lowered on devices");
    assert!(
        error.contains("kernel shift view edge policy is not lowered on devices (limes<reflect>)"),
        "{error}"
    );
}
