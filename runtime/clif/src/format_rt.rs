//! The `¶` format operator: one entry per subject kind. The spec arrives as a
//! text handle; the layout parser and renderers are [`crate::format_spec`].

use super::{display_fractus, exact_trap, join_wide, text_bytes, text_new, text_str};
use crate::format_spec::{
    FormatSpec, LayoutSpec, parse_format_spec, render_decimal, render_float, render_instans_preset,
    render_integer, render_magnus, render_text,
};

/// # Safety
///
/// `spec` is a live runtime text holding a validated spec.
unsafe fn layout(spec: *const u8) -> LayoutSpec {
    let text = text_str(unsafe { text_bytes(spec) });
    match parse_format_spec(&text) {
        Ok(FormatSpec::Layout(layout)) => layout,
        _ => exact_trap("format_value operand not admitted by ¶"),
    }
}

/// An integer (low half, high half of the exact value).
///
/// # Safety
///
/// `spec` is a live runtime text holding a validated spec.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_format_int(
    _arena: *const super::Arena,
    low: i64,
    high: i64,
    spec: *const u8,
) -> *const u8 {
    let layout = unsafe { layout(spec) };
    text_new(
        _arena,
        render_integer(join_wide(low, high), &layout).as_bytes(),
    )
}

/// A float; `narrow` displays at `f32` width.
///
/// # Safety
///
/// `spec` is a live runtime text holding a validated spec.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_format_float(
    _arena: *const super::Arena,
    value: f64,
    narrow: i32,
    spec: *const u8,
) -> *const u8 {
    let layout = unsafe { layout(spec) };
    #[allow(clippy::cast_possible_truncation)]
    let rendered = render_float(value, &layout, |magnitude| {
        if narrow == 0 {
            display_fractus(magnitude)
        } else {
            display_fractus(magnitude as f32)
        }
    });
    text_new(_arena, rendered.as_bytes())
}

/// A text.
///
/// # Safety
///
/// Both handles are live runtime texts and `spec` is validated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_format_text(
    _arena: *const super::Arena,
    value: *const u8,
    spec: *const u8,
) -> *const u8 {
    let layout = unsafe { layout(spec) };
    let value = text_str(unsafe { text_bytes(value) });
    text_new(_arena, render_text(&value, &layout).as_bytes())
}

/// A decimal handle.
///
/// # Safety
///
/// `value` is a decimal handle and `spec` a validated spec text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_format_dec(
    _arena: *const super::Arena,
    value: *const u8,
    spec: *const u8,
) -> *const u8 {
    let layout = unsafe { layout(spec) };
    let (carrier, scale) = unsafe { crate::dec::raw(value) };
    text_new(_arena, render_decimal(carrier, scale, &layout).as_bytes())
}

/// An unbounded integer handle.
///
/// # Safety
///
/// `value` is an inf handle and `spec` a validated spec text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_format_inf(
    _arena: *const super::Arena,
    value: *const u8,
    spec: *const u8,
) -> *const u8 {
    let layout = unsafe { layout(spec) };
    let big = unsafe { crate::inf::magnus(value) };
    text_new(_arena, render_magnus(big, &layout).as_bytes())
}

/// An instant handle through a named preset.
///
/// # Safety
///
/// `value` is an instant handle and `spec` a validated preset name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_format_ins(
    _arena: *const super::Arena,
    value: *const u8,
    spec: *const u8,
) -> *const u8 {
    let text = text_str(unsafe { text_bytes(spec) });
    let Ok(FormatSpec::Preset(preset)) = parse_format_spec(&text) else {
        exact_trap("format_value operand not admitted by ¶")
    };
    let iso = unsafe { crate::ins::text(value) };
    text_new(_arena, render_instans_preset(&iso, preset).as_bytes())
}
