//! Infallible text appending for `String` buffers.
//!
//! `write!` into a `String` returns a `fmt::Result` that can never be `Err`.
//! These helpers format and append without a `Result` to discard.

use std::fmt;

/// Append formatted arguments to `out`.
pub(crate) fn append_args(out: &mut String, args: fmt::Arguments<'_>) {
    out.push_str(&fmt::format(args));
}

/// Append formatted text to a `String` (the `write!` replacement).
macro_rules! push_fmt {
    ($out:expr, $($arg:tt)*) => {
        $crate::text::append_args(&mut $out, format_args!($($arg)*))
    };
}

/// Append formatted text and a newline to a `String` (the `writeln!` replacement).
macro_rules! push_line {
    ($out:expr, $($arg:tt)*) => {{
        $crate::text::append_args(&mut $out, format_args!($($arg)*));
        $out.push('\n');
    }};
}

pub(crate) use push_fmt;
pub(crate) use push_line;
