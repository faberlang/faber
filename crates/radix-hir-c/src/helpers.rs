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

use crate::ty::Elem;
use std::collections::BTreeSet;

/// One unit of support text. The variant order is the emission order, so a
/// helper always follows the helpers it calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Helper {
    Types,
    Trap,
    Text,
    Arena,
    Concat,
    IntText,
    ExactText,
    Join,
    ListIndex,
    Quote,
    List(Elem, ListOp),
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
            // These stand alone: the 64-bit fast path is plain `int64_t` and
            // the overflow builtins; a list part's needs depend on its element
            // and operation (`list_needs`).
            Helper::Types
            | Helper::Trap
            | Helper::AddO
            | Helper::SubO
            | Helper::MulO
            | Helper::FdivI64
            | Helper::FmodI64
            | Helper::Quote
            | Helper::List(_, _) => &[],
            Helper::Text => &[Helper::Types],
            Helper::Arena => &[Helper::Trap],
            Helper::Concat | Helper::IntText | Helper::Join => &[Helper::Arena],
            Helper::ExactText => &[Helper::Arena, Helper::Text],
            Helper::Cap | Helper::Div | Helper::FitI64 | Helper::ListIndex => {
                &[Helper::Types, Helper::Trap]
            }
            Helper::Add | Helper::Sub | Helper::Mul => &[Helper::Cap],
            Helper::Rem => &[Helper::Div],
            Helper::StoreI64 => &[Helper::Types, Helper::Text],
        }
    }

    fn text(self) -> String {
        if let Helper::List(elem, op) = self {
            return list_part(elem, op);
        }
        let text = match self {
            Helper::Types => TYPES,
            Helper::Trap => TRAP,
            Helper::Text => TEXT,
            Helper::Arena => ARENA,
            Helper::Concat => CONCAT,
            Helper::IntText => INT_TEXT,
            Helper::ExactText => EXACT_TEXT,
            Helper::Join => JOIN,
            Helper::ListIndex => LIST_INDEX,
            Helper::Quote => QUOTE,
            Helper::List(_, _) => "",
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
        };
        text.to_owned()
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

/// The arena (goal unit C5): chunks from `malloc`, bump allocation aligned to
/// `max_align_t`, nothing freed while the program runs. The root arena belongs
/// to `main` and reaches every function as the hidden first parameter
/// `faber_a`; there is no global current arena (goal open question 3).
const ARENA: &str = "\
typedef struct faber_chunk {
    struct faber_chunk *next;
    size_t size;
    size_t used;
    unsigned char data[];
} faber_chunk;

typedef struct faber_arena {
    faber_chunk *head;
} faber_arena;

static void *faber_alloc(faber_arena *arena, size_t size) {
    const size_t align = _Alignof(max_align_t);
    faber_chunk *chunk = arena->head;
    size_t capacity;
    if (chunk != NULL) {
        unsigned char *next = chunk->data + chunk->used;
        size_t pad = (align - (uintptr_t)next % align) % align;
        if (pad <= chunk->size - chunk->used && size <= chunk->size - chunk->used - pad) {
            chunk->used += pad + size;
            return next + pad;
        }
    }
    if (size > SIZE_MAX - sizeof(faber_chunk) - align) {
        faber_trap(\"out of memory\");
    }
    capacity = size + align > 65536 ? size + align : 65536;
    chunk = malloc(sizeof(faber_chunk) + capacity);
    if (chunk == NULL) {
        faber_trap(\"out of memory\");
    }
    chunk->next = arena->head;
    chunk->size = capacity;
    chunk->used = 0;
    arena->head = chunk;
    return faber_alloc(arena, size);
}
";

const CONCAT: &str = "\
static const char *faber_concat(faber_arena *arena, const char *a, const char *b) {
    size_t a_len = strlen(a);
    size_t b_len = strlen(b);
    char *out = faber_alloc(arena, a_len + b_len + 1);
    memcpy(out, a, a_len);
    memcpy(out + a_len, b, b_len + 1);
    return out;
}
";

const INT_TEXT: &str = "\
static const char *faber_int_text(faber_arena *arena, int64_t value) {
    char *out = faber_alloc(arena, 21);
    snprintf(out, 21, \"%\" PRId64, value);
    return out;
}
";

const EXACT_TEXT: &str = "\
static const char *faber_exact_text(faber_arena *arena, faber_i128 value) {
    return faber_i128_text(value, faber_alloc(arena, 41));
}
";

/// A template: `count` text pieces joined in one allocation.
const JOIN: &str = "\
static const char *faber_join(faber_arena *arena, int count, ...) {
    va_list pieces;
    size_t total = 1;
    char *out;
    char *cursor;
    int index;
    va_start(pieces, count);
    for (index = 0; index < count; index++) {
        total += strlen(va_arg(pieces, const char *));
    }
    va_end(pieces);
    out = faber_alloc(arena, total);
    cursor = out;
    va_start(pieces, count);
    for (index = 0; index < count; index++) {
        const char *piece = va_arg(pieces, const char *);
        size_t length = strlen(piece);
        memcpy(cursor, piece, length);
        cursor += length;
    }
    va_end(pieces);
    *cursor = '\\0';
    return out;
}
";

/// The index check every list read and write shares, with the runner's two
/// trap texts in the runner's order.
const LIST_INDEX: &str = "\
static int64_t faber_list_index(faber_i128 index, int64_t length) {
    if (index < 0) {
        faber_trap(\"expected non-negative numerus index\");
    }
    if (index >= length) {
        faber_trap(\"array index out of bounds\");
    }
    return (int64_t)index;
}
";

/// A text element as a list shows it: quoted, with the escapes the runner
/// prints (Rust's debug form: `\"`, `\\`, `\n`, `\t`, `\r`, `\u{..}` for
/// other control bytes; other bytes verbatim). Writes when `out` is not
/// NULL; returns the length either way.
const QUOTE: &str = "\
static size_t faber_quote(char *out, const char *text) {
    const unsigned char *cursor;
    size_t at = 0;
    for (cursor = (const unsigned char *)text; *cursor != 0; cursor++) {
        char escape[8];
        size_t length;
        switch (*cursor) {
        case '\"':
            memcpy(escape, \"\\\\\\\"\", 3);
            length = 2;
            break;
        case '\\\\':
            memcpy(escape, \"\\\\\\\\\", 3);
            length = 2;
            break;
        case '\\n':
            memcpy(escape, \"\\\\n\", 3);
            length = 2;
            break;
        case '\\t':
            memcpy(escape, \"\\\\t\", 3);
            length = 2;
            break;
        case '\\r':
            memcpy(escape, \"\\\\r\", 3);
            length = 2;
            break;
        default:
            if (*cursor < 0x20 || *cursor == 0x7f) {
                length = (size_t)snprintf(escape, sizeof escape, \"\\\\u{%x}\", (unsigned)*cursor);
            } else {
                escape[0] = (char)*cursor;
                length = 1;
            }
            break;
        }
        if (out != NULL) {
            memcpy(out + at, escape, length);
        }
        at += length;
    }
    return at;
}
";

/// One operation of a list family. Every part comes from one template per
/// operation, so the element kinds cannot drift, and only the parts a program
/// uses are emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ListOp {
    Struct,
    New,
    Of,
    Push,
    At,
    Set,
    Show,
}

/// The helpers one list part calls or names.
fn list_needs(elem: Elem, op: ListOp) -> Vec<Helper> {
    let own = |op| Helper::List(elem, op);
    match op {
        ListOp::Struct => Vec::new(),
        ListOp::Of => vec![own(ListOp::New)],
        ListOp::At | ListOp::Set => vec![own(ListOp::Struct), Helper::ListIndex],
        ListOp::Show if elem == Elem::Text => {
            vec![own(ListOp::Struct), Helper::Arena, Helper::Quote]
        }
        ListOp::New | ListOp::Push | ListOp::Show => vec![own(ListOp::Struct), Helper::Arena],
    }
}

fn list_part(elem: Elem, op: ListOp) -> String {
    let n = elem.suffix();
    let t = elem.c_type();
    match op {
        ListOp::Struct => {
            let text_type = if elem == Elem::Text {
                "typedef const char *faber_text;\n\n"
            } else {
                ""
            };
            format!(
                "{text_type}typedef struct faber_list_{n} {{
    {t} *items;
    int64_t len;
    int64_t cap;
}} faber_list_{n};
"
            )
        }
        ListOp::New => format!(
            "static faber_list_{n} *faber_list_{n}_new(faber_arena *arena, int64_t capacity) {{
    faber_list_{n} *list = faber_alloc(arena, sizeof *list);
    list->items = capacity > 0 ? faber_alloc(arena, (size_t)capacity * sizeof *list->items) : NULL;
    list->len = 0;
    list->cap = capacity;
    return list;
}}
"
        ),
        ListOp::Of => format!(
            "static faber_list_{n} *faber_list_{n}_of(faber_arena *arena, int64_t count, const {t} *items) {{
    faber_list_{n} *list = faber_list_{n}_new(arena, count);
    memcpy(list->items, items, (size_t)count * sizeof *items);
    list->len = count;
    return list;
}}
"
        ),
        ListOp::Push => format!(
            "static void faber_list_{n}_push(faber_arena *arena, faber_list_{n} *list, {t} item) {{
    if (list->len == list->cap) {{
        int64_t capacity = list->cap < 4 ? 4 : list->cap * 2;
        {t} *items = faber_alloc(arena, (size_t)capacity * sizeof *items);
        if (list->len > 0) {{
            memcpy(items, list->items, (size_t)list->len * sizeof *items);
        }}
        list->items = items;
        list->cap = capacity;
    }}
    list->items[list->len++] = item;
}}
"
        ),
        ListOp::At => format!(
            "static {t} faber_list_{n}_at(const faber_list_{n} *list, faber_i128 index) {{
    return list->items[faber_list_index(index, list->len)];
}}
"
        ),
        ListOp::Set => format!(
            "static void faber_list_{n}_set(faber_list_{n} *list, faber_i128 index, {t} item) {{
    list->items[faber_list_index(index, list->len)] = item;
}}
"
        ),
        ListOp::Show => list_show(elem),
    }
}

/// How a list shows, as the runner prints it.
fn list_show(elem: Elem) -> String {
    let n = elem.suffix();
    match elem {
        Elem::Int => format!(
            "static const char *faber_list_{n}_show(faber_arena *arena, const faber_list_{n} *list) {{
    size_t capacity = 3 + (size_t)list->len * 22;
    char *out = faber_alloc(arena, capacity);
    size_t at = 0;
    int64_t index;
    out[at++] = '[';
    for (index = 0; index < list->len; index++) {{
        if (index > 0) {{
            out[at++] = ',';
            out[at++] = ' ';
        }}
        at += (size_t)snprintf(out + at, capacity - at, \"%\" PRId64, list->items[index]);
    }}
    out[at++] = ']';
    out[at] = '\\0';
    return out;
}}
"
        ),
        Elem::Text => format!(
            "static const char *faber_list_{n}_show(faber_arena *arena, const faber_list_{n} *list) {{
    size_t total = 3;
    char *out;
    size_t at = 0;
    int64_t index;
    for (index = 0; index < list->len; index++) {{
        total += faber_quote(NULL, list->items[index]) + 4;
    }}
    out = faber_alloc(arena, total);
    out[at++] = '[';
    for (index = 0; index < list->len; index++) {{
        if (index > 0) {{
            out[at++] = ',';
            out[at++] = ' ';
        }}
        out[at++] = '\"';
        at += faber_quote(out + at, list->items[index]);
        out[at++] = '\"';
    }}
    out[at++] = ']';
    out[at] = '\\0';
    return out;
}}
"
        ),
        Elem::Bool => format!(
            "static const char *faber_list_{n}_show(faber_arena *arena, const faber_list_{n} *list, const char *yes, const char *no) {{
    size_t yes_length = strlen(yes);
    size_t no_length = strlen(no);
    size_t width = yes_length > no_length ? yes_length : no_length;
    char *out = faber_alloc(arena, 3 + (size_t)list->len * (width + 2));
    size_t at = 0;
    int64_t index;
    out[at++] = '[';
    for (index = 0; index < list->len; index++) {{
        const char *word = list->items[index] ? yes : no;
        size_t length = list->items[index] ? yes_length : no_length;
        if (index > 0) {{
            out[at++] = ',';
            out[at++] = ' ';
        }}
        memcpy(out + at, word, length);
        at += length;
    }}
    out[at++] = ']';
    out[at] = '\\0';
    return out;
}}
"
        ),
    }
}

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
    /// `int64_t`, `INT64_C`, `INT64_MIN`, `UINT64_MAX`, `uintptr_t`,
    /// `SIZE_MAX` and `PRId64`; `<inttypes.h>` includes `<stdint.h>`.
    Inttypes,
    /// `va_list`.
    Stdarg,
    /// `bool`.
    Stdbool,
    /// `size_t`, `max_align_t`.
    Stddef,
    /// `printf`, `fprintf`, `snprintf`.
    Stdio,
    /// `exit`, `malloc`.
    Stdlib,
    /// `strlen`, `memcpy`.
    String,
}

impl Header {
    fn include(self) -> &'static str {
        match self {
            Header::Inttypes => "#include <inttypes.h>\n",
            Header::Stdarg => "#include <stdarg.h>\n",
            Header::Stdbool => "#include <stdbool.h>\n",
            Header::Stddef => "#include <stddef.h>\n",
            Header::Stdio => "#include <stdio.h>\n",
            Header::Stdlib => "#include <stdlib.h>\n",
            Header::String => "#include <string.h>\n",
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
        let headers: &[Header] = match helper {
            Helper::Trap | Helper::StoreI64 => &[Header::Stdio, Header::Stdlib],
            Helper::Arena => &[Header::Stddef, Header::Stdlib],
            Helper::Concat => &[Header::String],
            Helper::IntText => &[Header::Stdio],
            Helper::Join => &[Header::Stdarg, Header::String],
            Helper::Quote | Helper::List(_, ListOp::Of | ListOp::Push | ListOp::Show) => {
                &[Header::Stdio, Header::String]
            }
            _ => &[],
        };
        self.headers.extend(headers.iter().copied());
        for dependency in helper.needs() {
            self.need(*dependency);
        }
        if let Helper::List(elem, op) = helper {
            for dependency in list_needs(elem, op) {
                self.need(dependency);
            }
        }
    }

    /// The program allocates, so every function carries the arena.
    pub(crate) fn uses_arena(&self) -> bool {
        self.helpers.contains(&Helper::Arena)
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
            out.push_str(&helper.text());
            out.push('\n');
        }
        out
    }
}
