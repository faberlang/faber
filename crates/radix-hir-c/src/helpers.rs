//! Inline-on-use program text: the headers and helper functions a program
//! turned out to need.
//!
//! The exact integer model mirrors the Rust target's `faber::exact` runtime
//! and the MIR runner: an integer intermediate is a `faber_i128` inside the
//! representation cap `[-2^63, 2^64 - 1]` (ruling 17a), checked after every
//! operation; `/` and `%` floor; the 64-bit width applies once, at the store.
//! A trap writes `error: <reason>` to stderr and exits with status 1, as the
//! runner does. Nothing is emitted unless the program uses it, and there is no
//! runtime library.
//!
//! TARGET: c (GCC or Clang). `__int128` and `__builtin_mul_overflow` are
//! extensions; `__extension__` keeps `-pedantic` quiet about the typedefs.

use std::collections::BTreeSet;

/// One unit of support text. The variant order is the emission order, so a
/// helper always follows the helpers it calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Helper {
    Types,
    Trap,
    Text,
    Cap,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    FitI64,
    StoreI64,
    AddO,
    SubO,
    MulO,
    FdivI64,
    FmodI64,
}

impl Helper {
    /// Helpers this one calls or names.
    fn needs(self) -> &'static [Helper] {
        match self {
            // The 64-bit fast-path helpers stand alone: plain `int64_t` and
            // the overflow builtins.
            Helper::Types
            | Helper::Trap
            | Helper::AddO
            | Helper::SubO
            | Helper::MulO
            | Helper::FdivI64
            | Helper::FmodI64 => &[],
            Helper::Text => &[Helper::Types],
            Helper::Cap | Helper::Div | Helper::FitI64 => &[Helper::Types, Helper::Trap],
            Helper::Add | Helper::Sub | Helper::Mul => &[Helper::Cap],
            Helper::Rem => &[Helper::Div],
            Helper::StoreI64 => &[Helper::Types, Helper::Text],
        }
    }

    fn text(self) -> &'static str {
        match self {
            Helper::Types => TYPES,
            Helper::Trap => TRAP,
            Helper::Text => TEXT,
            Helper::Cap => CAP,
            Helper::Add => ADD,
            Helper::Sub => SUB,
            Helper::Mul => MUL,
            Helper::Div => DIV,
            Helper::Rem => REM,
            Helper::FitI64 => FIT_I64,
            Helper::StoreI64 => STORE_I64,
            Helper::AddO => ADD_O,
            Helper::SubO => SUB_O,
            Helper::MulO => MUL_O,
            Helper::FdivI64 => FDIV_I64,
            Helper::FmodI64 => FMOD_I64,
        }
    }
}

const TYPES: &str = "\
__extension__ typedef __int128 faber_i128;
__extension__ typedef unsigned __int128 faber_u128;
";

const TRAP: &str = "\
static _Noreturn void faber_trap(const char *reason) {
    fprintf(stderr, \"error: %s\\n\", reason);
    exit(1);
}
";

/// Decimal text of an exact value into a caller buffer of at least 41 bytes
/// (39 digits, a sign and the terminator).
const TEXT: &str = "\
static const char *faber_i128_text(faber_i128 value, char *buffer) {
    char *cursor = buffer + 40;
    faber_u128 magnitude = value < 0 ? -(faber_u128)value : (faber_u128)value;
    *cursor = '\\0';
    do {
        *--cursor = (char)('0' + (int)(magnitude % 10));
        magnitude /= 10;
    } while (magnitude != 0);
    if (value < 0) {
        *--cursor = '-';
    }
    return cursor;
}
";

/// Every operand of the arithmetic helpers is an `int64_t`, a literal of at
/// most `UINT64_MAX`, or a capped result, so a sum or difference of two cannot
/// overflow `faber_i128`; a product can, hence the builtin.
const CAP: &str = "\
static faber_i128 faber_cap(faber_i128 value) {
    if (value < INT64_MIN || value > (faber_i128)UINT64_MAX) {
        faber_trap(\"numerus overflow\");
    }
    return value;
}
";

const ADD: &str = "\
static faber_i128 faber_add(faber_i128 a, faber_i128 b) {
    return faber_cap(a + b);
}
";

const SUB: &str = "\
static faber_i128 faber_sub(faber_i128 a, faber_i128 b) {
    return faber_cap(a - b);
}
";

const MUL: &str = "\
static faber_i128 faber_mul(faber_i128 a, faber_i128 b) {
    faber_i128 product;
    if (__builtin_mul_overflow(a, b, &product)) {
        faber_trap(\"numerus overflow\");
    }
    return faber_cap(product);
}
";

/// Floor division. Operands lie in the cap, so the quotient does too (the
/// extreme is `-2^63 / -1 = 2^63`).
const DIV: &str = "\
static faber_i128 faber_div(faber_i128 a, faber_i128 b) {
    faber_i128 quotient;
    if (b == 0) {
        faber_trap(\"numerus division failed\");
    }
    quotient = a / b;
    if (a % b != 0 && ((a < 0) != (b < 0))) {
        quotient -= 1;
    }
    return quotient;
}
";

/// Floor remainder: the divisor's sign.
const REM: &str = "\
static faber_i128 faber_rem(faber_i128 a, faber_i128 b) {
    return a - b * faber_div(a, b);
}
";

/// Narrowing an exact value to its own expression type where typecheck put no
/// store node, because its range rule proved the result fits (`big + 1 - 1`
/// is `int` though `big + 1` is not). The check cannot fail on a proven range;
/// it stays so a wrong proof traps instead of truncating. Same text as the
/// Rust target's `faber::exact::fit`.
const FIT_I64: &str = "\
static int64_t faber_fit_i64(faber_i128 value) {
    if (value < INT64_MIN || value > INT64_MAX) {
        faber_trap(\"numerus overflow\");
    }
    return (int64_t)value;
}
";

/// The store into an `int` (`int64_t`) cell: the one place the width applies.
const STORE_I64: &str = "\
static int64_t faber_store_i64(faber_i128 value, const char *at) {
    if (value < INT64_MIN || value > INT64_MAX) {
        char text[41];
        fprintf(stderr, \"error: %s does not fit in `i64` (%s)\\n\", faber_i128_text(value, text), at);
        exit(1);
    }
    return (int64_t)value;
}
";

/// The 64-bit fast path (goal unit C4). Each operation computes the wrapped
/// `int64_t` result, which the builtins define, and records an overflow in
/// the caller's flag. When the flag stays clear every exact intermediate
/// equals its `int64_t` value, so the fast result is the exact result; when it
/// is set, the caller re-evaluates the same pure expression through the exact
/// helpers, which give the exact value, trap and trap text.
const ADD_O: &str = "\
static inline int64_t faber_add_o(int64_t a, int64_t b, int *overflow) {
    int64_t sum;
    *overflow |= __builtin_add_overflow(a, b, &sum);
    return sum;
}
";

const SUB_O: &str = "\
static inline int64_t faber_sub_o(int64_t a, int64_t b, int *overflow) {
    int64_t difference;
    *overflow |= __builtin_sub_overflow(a, b, &difference);
    return difference;
}
";

const MUL_O: &str = "\
static inline int64_t faber_mul_o(int64_t a, int64_t b, int *overflow) {
    int64_t product;
    *overflow |= __builtin_mul_overflow(a, b, &product);
    return product;
}
";

/// Floor division of an `int64_t` by a constant other than 0 and -1: C's `/`
/// cannot overflow, and the floored quotient stays inside `int64_t`
/// (`|b| >= 2` halves the magnitude; `b == 1` never adjusts).
const FDIV_I64: &str = "\
static inline int64_t faber_fdiv_i64(int64_t a, int64_t b) {
    int64_t quotient = a / b;
    if (a % b != 0 && ((a < 0) != (b < 0))) {
        quotient -= 1;
    }
    return quotient;
}
";

/// Floor remainder by a constant other than 0 and -1, from C's `%` so no
/// product can overflow: the result takes the divisor's sign.
const FMOD_I64: &str = "\
static inline int64_t faber_fmod_i64(int64_t a, int64_t b) {
    int64_t remainder = a % b;
    if (remainder != 0 && ((remainder < 0) != (b < 0))) {
        remainder += b;
    }
    return remainder;
}
";

/// A standard header. The variant order is the `#include` order
/// (alphabetical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Header {
    /// `int64_t`, `INT64_C`, `INT64_MIN`, `UINT64_MAX` and `PRId64`;
    /// `<inttypes.h>` includes `<stdint.h>`.
    Inttypes,
    /// `bool`.
    Stdbool,
    /// `printf`, `fprintf`.
    Stdio,
    /// `exit`.
    Stdlib,
}

impl Header {
    fn include(self) -> &'static str {
        match self {
            Header::Inttypes => "#include <inttypes.h>\n",
            Header::Stdbool => "#include <stdbool.h>\n",
            Header::Stdio => "#include <stdio.h>\n",
            Header::Stdlib => "#include <stdlib.h>\n",
        }
    }
}

/// Headers and helpers the emitted program needs, filled while emitting.
#[derive(Default)]
pub(crate) struct Support {
    headers: BTreeSet<Header>,
    helpers: BTreeSet<Helper>,
}

impl Support {
    /// The program declares or prints a 64-bit integer.
    pub(crate) fn need_ints(&mut self) {
        self.headers.insert(Header::Inttypes);
    }

    /// The program prints through `printf`.
    pub(crate) fn need_stdio(&mut self) {
        self.headers.insert(Header::Stdio);
    }

    /// The program declares a `bool`.
    pub(crate) fn need_bool(&mut self) {
        self.headers.insert(Header::Stdbool);
    }

    /// The program calls `helper`, and so everything `helper` calls.
    pub(crate) fn need(&mut self, helper: Helper) {
        if !self.helpers.insert(helper) {
            return;
        }
        self.headers.insert(Header::Inttypes);
        if matches!(helper, Helper::Trap | Helper::StoreI64) {
            self.headers.insert(Header::Stdio);
            self.headers.insert(Header::Stdlib);
        }
        for dependency in helper.needs() {
            self.need(*dependency);
        }
    }

    /// The `#include` lines in alphabetical order, then the helpers in
    /// dependency order, each block followed by a blank line; empty when the
    /// program needs nothing.
    pub(crate) fn text(&self) -> String {
        let mut out = String::new();
        for header in &self.headers {
            out.push_str(header.include());
        }
        if !out.is_empty() {
            out.push('\n');
        }
        for helper in &self.helpers {
            out.push_str(helper.text());
            out.push('\n');
        }
        out
    }
}
