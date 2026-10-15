//! Inline-on-use program text: the headers a program turned out to need.
//!
//! Helper functions (exact arithmetic, store checks) are added here as the
//! exact integer model lands; nothing is emitted unless the program uses it,
//! and there is no runtime library.

/// Headers the emitted program needs, filled while emitting.
#[derive(Default)]
pub(crate) struct Includes {
    /// `int64_t`, `INT64_C`, `INT64_MIN` and `PRId64`; `<inttypes.h>`
    /// includes `<stdint.h>`.
    ints: bool,
    /// `printf`.
    stdio: bool,
}

impl Includes {
    /// The program declares or prints a 64-bit integer.
    pub(crate) fn need_ints(&mut self) {
        self.ints = true;
    }

    /// The program prints through `printf`.
    pub(crate) fn need_stdio(&mut self) {
        self.stdio = true;
    }

    /// The `#include` lines in alphabetical order, with a trailing blank line;
    /// empty when the program includes nothing.
    pub(crate) fn text(&self) -> String {
        let mut out = String::new();
        if self.ints {
            out.push_str("#include <inttypes.h>\n");
        }
        if self.stdio {
            out.push_str("#include <stdio.h>\n");
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out
    }
}
