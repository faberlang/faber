//! The canonical runtime-helper table, its per-target spelling rule, the
//! reserved-name set and the emission-time import recorder.
//!
//! Every helper a generated program calls lives in a per-target runtime
//! package. A helper's canonical id is `<namespace>.<name>`: both ASCII
//! `lower_snake_case`, except a type, whose name is `UpperCamelCase`. One rule
//! per target derives the emitted spelling from that id, so no emitter hand
//! spells a helper:
//!
//! | Target | Function | Type |
//! | --- | --- | --- |
//! | Rust | `faber::<ns>::<name>` | `faber::<Name>` |
//! | TypeScript | `<ns>.<lowerCamel(name)>` | `<ns>.<UpperCamel(name)>` |
//! | Go | `rt.<UpperCamel(ns)><UpperCamel(name)>` | same |
//! | Swift | `<UpperCamel(ns)>.<lowerCamel(name)>` | `<UpperCamel(ns)>.<UpperCamel(name)>` |
//! | Python | `<ns>.<name>` | `<ns>.<UpperCamel(name)>` |
//!
//! The Go rule is flat, so two ids could in principle spell alike
//! (`exact`+`store_i` against a hypothetical `exact_store`+`i`); the tests
//! assert that every target's spelling is injective over a slice.
//!
//! A row carries no body, parameter list or trap text: bodies are target code
//! in each runtime package. A row exists only for a helper some runtime package
//! carries, so each namespace unit adds its rows to the real [`TABLE`] with the
//! runtime it moves.
//!
//! Store-family helpers (`StoreFn`) enforce a width limit and may only be
//! spelled at a store site: [`spell`] refuses them and [`spell_store`], which
//! demands the [`HirStoreSite`], is the one way to spell one.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BTreeSet;

use radix_hir::HirStoreSite;
use radix_target::Target;

/// The Go import path of the flat runtime package (`package rt`).
pub const GO_RUNTIME_IMPORT: &str = "faber/rt";

/// The TypeScript module specifier of the runtime package (`@faber/runtime`):
/// each namespace it exports is imported by name from here.
pub const TS_RUNTIME_IMPORT: &str = "@faber/runtime";

/// `display.verum`: a bivalens as `verum` / `falsum`.
pub const DISPLAY_VERUM: RuntimeItem = RuntimeItem {
    ns: Namespace::Display,
    name: "verum",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirGo],
};

/// `display.list`: a list as `[a, b, c]` through an element renderer (an
/// element hint in the TypeScript runtime).
pub const DISPLAY_LIST: RuntimeItem = RuntimeItem {
    ns: Namespace::Display,
    name: "list",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirGo, Target::HirTypeScript],
};

/// `display.valor`: a boxed valor / json value with Faber display semantics.
pub const DISPLAY_VALOR: RuntimeItem = RuntimeItem {
    ns: Namespace::Display,
    name: "valor",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirGo, Target::HirRust, Target::HirTypeScript],
};

/// The targets whose runtime package carries the Rust-only `display` rows.
const DISPLAY_RUST: &[Target] = &[Target::HirRust];

const fn display_rust_row(name: &'static str) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Display,
        name,
        kind: RuntimeKind::Fn,
        targets: DISPLAY_RUST,
    }
}

/// `display` rows of the Rust runtime (T1-R5, `faber::display`): scalar and
/// option renderers a `nota`/interpolation site calls with Faber display
/// semantics (`verum`/`falsum`, `nihil`, a `.0` marker on a whole float).
pub const DISPLAY_BIVALENS: RuntimeItem = display_rust_row("bivalens");
/// `display.fractus`: an f64 as text (Rust and TypeScript runtimes).
pub const DISPLAY_FRACTUS: RuntimeItem = RuntimeItem {
    ns: Namespace::Display,
    name: "fractus",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirRust, Target::HirTypeScript],
};
pub const DISPLAY_OPTION: RuntimeItem = display_rust_row("option");
pub const DISPLAY_OPTION_BIVALENS: RuntimeItem = display_rust_row("option_bivalens");
pub const DISPLAY_OPTION_FRACTUS: RuntimeItem = display_rust_row("option_fractus");
pub const DISPLAY_OPTION_VACUUM: RuntimeItem = display_rust_row("option_vacuum");
pub const DISPLAY_TEXT_PAYLOAD: RuntimeItem = display_rust_row("text_payload");
/// The `*_with` siblings of the Rust display renderers: each takes the
/// module's `DisplayTokens` as a last argument (the print locale follows the
/// code locale). Generated code calls them only for a non-Latin token set.
pub const DISPLAY_BIVALENS_WITH: RuntimeItem = display_rust_row("bivalens_with");
pub const DISPLAY_OPTION_WITH: RuntimeItem = display_rust_row("option_with");
pub const DISPLAY_OPTION_BIVALENS_WITH: RuntimeItem = display_rust_row("option_bivalens_with");
pub const DISPLAY_OPTION_FRACTUS_WITH: RuntimeItem = display_rust_row("option_fractus_with");
pub const DISPLAY_OPTION_VACUUM_WITH: RuntimeItem = display_rust_row("option_vacuum_with");
/// `display.valor_with`: `valor` printing the module's display tokens.
pub const DISPLAY_VALOR_WITH: RuntimeItem = display_rust_row("valor_with");
/// `display.DisplayTokens`: the output words of one locale (Rust runtime; the
/// type stays in `faber::display`, see [`spelling`]).
pub const DISPLAY_DISPLAY_TOKENS: RuntimeItem = RuntimeItem {
    ns: Namespace::Display,
    name: "DisplayTokens",
    kind: RuntimeKind::Type,
    targets: DISPLAY_RUST,
};

const DISPLAY_TS: &[Target] = &[Target::HirTypeScript];

const fn display_ts_row(name: &'static str) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Display,
        name,
        kind: RuntimeKind::Fn,
        targets: DISPLAY_TS,
    }
}

/// `display` rows of the TypeScript runtime (T1-T1, `display` in
/// `@faber/runtime`): `value` renders any value under a display hint and is the
/// one call a `nota`/interpolation site spells; the other renderers it
/// dispatches to are rows too, because the package carries them. `fractus`,
/// `list` and `valor` are the shared rows above.
pub const DISPLAY_VALUE: RuntimeItem = display_ts_row("value");
pub const DISPLAY_NUMERUS: RuntimeItem = display_ts_row("numerus");
pub const DISPLAY_TENSOR: RuntimeItem = display_ts_row("tensor");
pub const DISPLAY_SPARSA: RuntimeItem = display_ts_row("sparsa");
pub const DISPLAY_MAP: RuntimeItem = display_ts_row("map");
pub const DISPLAY_TAGGED_VALOR: RuntimeItem = display_ts_row("tagged_valor");

/// The targets whose runtime package carries an `exact` row: the Rust runtime
/// carries the rows below, the Go runtime (`rt.Exact*`) carries its own rows
/// and shares the rows whose id and meaning match (`add`, `sub`, `mul`, `div`,
/// `shl`, `shr`, `pow`, `store`), and the TypeScript runtime (`exact` in
/// `@faber/runtime`, T1-T2) shares every row whose id and meaning match and
/// carries the rest as its own.
const EXACT_RUST: &[Target] = &[Target::HirRust];
const EXACT_RUST_GO_TS: &[Target] = &[Target::HirRust, Target::HirTypeScript, Target::HirGo];
const EXACT_GO_TS: &[Target] = &[Target::HirTypeScript, Target::HirGo];
const EXACT_GO: &[Target] = &[Target::HirGo];
const EXACT_TS: &[Target] = &[Target::HirTypeScript];

const fn exact_row(name: &'static str, kind: RuntimeKind) -> RuntimeItem {
    exact_row_for(name, kind, EXACT_RUST)
}

const fn exact_row_for(
    name: &'static str,
    kind: RuntimeKind,
    targets: &'static [Target],
) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Exact,
        name,
        kind,
        targets,
    }
}

/// `exact` rows (T1-R1): the 64-bit-window integer carrier of the Rust runtime
/// (`faber::exact`). `EXACT_STORE` and `EXACT_STORE_INFERRED` are the only
/// store-family rows: the width limit applies at a store site and nowhere else.
/// `EXACT_ADD`, `EXACT_SUB` and `EXACT_MUL` (T1-R6) check the representation
/// window only, so they are plain rows (an unstored computation has no store
/// site to witness).
pub const EXACT_ADD: RuntimeItem = exact_row_for("add", RuntimeKind::Fn, EXACT_RUST_GO_TS);
pub const EXACT_APPROX_INT: RuntimeItem = exact_row("approx_int", RuntimeKind::Fn);
pub const EXACT_APPROX_F32: RuntimeItem = exact_row("approx_f32", RuntimeKind::Fn);
pub const EXACT_APPROX_F64: RuntimeItem = exact_row("approx_f64", RuntimeKind::Fn);
pub const EXACT_CMP_INT_FLOAT: RuntimeItem = exact_row("cmp_int_float", RuntimeKind::Fn);
pub const EXACT_CMP_SCALED_FLOAT: RuntimeItem = exact_row("cmp_scaled_float", RuntimeKind::Fn);
pub const EXACT_DIV: RuntimeItem = exact_row_for("div", RuntimeKind::Fn, EXACT_RUST_GO_TS);
pub const EXACT_FIT: RuntimeItem = exact_row("fit", RuntimeKind::Fn);
pub const EXACT_INT: RuntimeItem = exact_row("int", RuntimeKind::Fn);
pub const EXACT_MUL: RuntimeItem = exact_row_for("mul", RuntimeKind::Fn, EXACT_RUST_GO_TS);
pub const EXACT_POW: RuntimeItem = exact_row_for("pow", RuntimeKind::Fn, EXACT_RUST_GO_TS);
pub const EXACT_REM: RuntimeItem = exact_row("rem", RuntimeKind::Fn);
pub const EXACT_SHL: RuntimeItem = exact_row_for("shl", RuntimeKind::Fn, EXACT_RUST_GO_TS);
pub const EXACT_SHR: RuntimeItem = exact_row_for("shr", RuntimeKind::Fn, EXACT_RUST_GO_TS);
pub const EXACT_STORE: RuntimeItem = exact_row_for("store", RuntimeKind::StoreFn, EXACT_RUST_GO_TS);
pub const EXACT_STORE_INFERRED: RuntimeItem = exact_row("store_inferred", RuntimeKind::StoreFn);
pub const EXACT_SUB: RuntimeItem = exact_row_for("sub", RuntimeKind::Fn, EXACT_RUST_GO_TS);

/// `exact` rows (T1-G2): the Go carrier of the exact integer intermediate
/// (`rt.Exact*`): the sign-magnitude `ExactX{neg, m}` over `[-2^63, 2^64 - 1]`.
/// The Go runtime carries `div`, `shl`, `shr`, `pow` and the trapping `store`
/// through the shared rows above (`rt.ExactStore[int64](x, pos)`: the generic
/// instantiation names the width). The width limit applies only in the store
/// family (`store`, `wrap`, `clamp_i`, `clamp_u`): a value leaves the carrier
/// into a bounded slot there and nowhere else.
pub const EXACT_X: RuntimeItem = exact_row_for("X", RuntimeKind::Type, EXACT_GO);
pub const EXACT_MK: RuntimeItem = exact_row_for("mk", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_I: RuntimeItem = exact_row_for("i", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_U: RuntimeItem = exact_row_for("u", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_NEG: RuntimeItem = exact_row_for("neg", RuntimeKind::Fn, EXACT_GO_TS);
pub const EXACT_ADD_S: RuntimeItem = exact_row_for("add_s", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_MOD: RuntimeItem = exact_row_for("mod", RuntimeKind::Fn, EXACT_GO_TS);
pub const EXACT_W: RuntimeItem = exact_row_for("w", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_FROM_W: RuntimeItem = exact_row_for("from_w", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_AND: RuntimeItem = exact_row_for("and", RuntimeKind::Fn, EXACT_GO_TS);
pub const EXACT_OR: RuntimeItem = exact_row_for("or", RuntimeKind::Fn, EXACT_GO_TS);
pub const EXACT_XOR: RuntimeItem = exact_row_for("xor", RuntimeKind::Fn, EXACT_GO_TS);
pub const EXACT_NOT: RuntimeItem = exact_row_for("not", RuntimeKind::Fn, EXACT_GO_TS);
pub const EXACT_ABS: RuntimeItem = exact_row_for("abs", RuntimeKind::Fn, EXACT_GO_TS);
pub const EXACT_CMP: RuntimeItem = exact_row_for("cmp", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_APPROX: RuntimeItem = exact_row_for("approx", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_CMP_F: RuntimeItem = exact_row_for("cmp_f", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_LT_F: RuntimeItem = exact_row_for("lt_f", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_LE_F: RuntimeItem = exact_row_for("le_f", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_GT_F: RuntimeItem = exact_row_for("gt_f", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_GE_F: RuntimeItem = exact_row_for("ge_f", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_EQ_F: RuntimeItem = exact_row_for("eq_f", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_STR: RuntimeItem = exact_row_for("str", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_TRAP: RuntimeItem = exact_row_for("trap", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_FLOAT: RuntimeItem = exact_row_for("float", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_FITS_I: RuntimeItem = exact_row_for("fits_i", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_FITS_U: RuntimeItem = exact_row_for("fits_u", RuntimeKind::Fn, EXACT_GO);
pub const EXACT_WRAP: RuntimeItem = exact_row_for("wrap", RuntimeKind::StoreFn, EXACT_GO_TS);
pub const EXACT_CLAMP_I: RuntimeItem = exact_row_for("clamp_i", RuntimeKind::StoreFn, EXACT_GO);
pub const EXACT_CLAMP_U: RuntimeItem = exact_row_for("clamp_u", RuntimeKind::StoreFn, EXACT_GO);

/// `exact` rows (T1-T2): the TypeScript carrier of the exact integer
/// (`exact` in `@faber/runtime`), a JS `number` held exact within
/// ±(2^53 - 1). The rows shared above (`add`, `sub`, `mul`, `div`, `mod`, `shl`,
/// `shr`, `pow`, `neg`, `not`, `abs`, `and`, `or`, `xor`, `store`, `wrap`) are
/// spelled the same way; the rows below exist only in the TypeScript runtime.
/// `chk` is the range check every checked helper ends in, `hi`, `lo` and `join`
/// are the split the bitwise operators go through, `float_rem` is the float
/// floor remainder, and `true_div` / `true_div_f32` are integer `÷`. `clamp` is
/// the saturating store: with `store` and `wrap`, the only place a width limit
/// applies.
pub const EXACT_CHK: RuntimeItem = exact_row_for("chk", RuntimeKind::Fn, EXACT_TS);
pub const EXACT_HI: RuntimeItem = exact_row_for("hi", RuntimeKind::Fn, EXACT_TS);
pub const EXACT_LO: RuntimeItem = exact_row_for("lo", RuntimeKind::Fn, EXACT_TS);
pub const EXACT_JOIN: RuntimeItem = exact_row_for("join", RuntimeKind::Fn, EXACT_TS);
pub const EXACT_FLOAT_REM: RuntimeItem = exact_row_for("float_rem", RuntimeKind::Fn, EXACT_TS);
pub const EXACT_TRUE_DIV: RuntimeItem = exact_row_for("true_div", RuntimeKind::Fn, EXACT_TS);
pub const EXACT_TRUE_DIV_F32: RuntimeItem =
    exact_row_for("true_div_f32", RuntimeKind::Fn, EXACT_TS);
pub const EXACT_CLAMP: RuntimeItem = exact_row_for("clamp", RuntimeKind::StoreFn, EXACT_TS);

/// The targets whose runtime package carries each `dec` row. The Rust runtime
/// (`faber::dec`), the Go runtime (`rt.Dec*`) and the TypeScript runtime
/// (`dec` in `@faber/runtime`, T1-T3) share the rows whose id and meaning
/// match; each also carries rows of its own.
const DEC_RUST: &[Target] = &[Target::HirRust];
const DEC_GO: &[Target] = &[Target::HirGo];
const DEC_RUST_GO_TS: &[Target] = &[Target::HirRust, Target::HirTypeScript, Target::HirGo];
const DEC_RUST_TS: &[Target] = &[Target::HirRust, Target::HirTypeScript];
const DEC_GO_TS: &[Target] = &[Target::HirTypeScript, Target::HirGo];

const fn dec_row(name: &'static str, kind: RuntimeKind, targets: &'static [Target]) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Dec,
        name,
        kind,
        targets,
    }
}

/// `dec` rows (T1-R2, T1-G3): the exact decimal (`d64`) carrier of the Rust
/// runtime (`faber::dec`, an `(i128, u32)` carrier/scale pair) and of the Go
/// runtime (`rt.Dec*`, a `math/big` coefficient at its own scale,
/// `DecCarrier`). `STORE` and `TRY_STORE` round an unstored decimal half-even
/// to scale 8 into a stored `i64`. Both are plain rows, not `StoreFn`: they are
/// spelled where an unstored decimal computation is consumed as a stored
/// carrier, which has no `HirStoreSite` at the call (a negation, a list
/// element, a binary result). Go's `TRY_PARSE` returns the runner's failure
/// message (empty on success); `PARSE` is the arm that panics with it. The
/// TypeScript runtime (`dec`, T1-T3) carries the same decimal as a
/// `[bigint, number]` pair; its `try_parse` returns `undefined` for text it
/// rejects, `parse` throws, `float_cmp` and `to_int` are shared with Go, and
/// `fmt_list` is shared with Rust.
pub const DEC_CARRIER: RuntimeItem = dec_row("Carrier", RuntimeKind::Type, DEC_GO);
pub const DEC_ADD: RuntimeItem = dec_row("add", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_CMP: RuntimeItem = dec_row("cmp", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_DIV: RuntimeItem = dec_row("div", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_F64: RuntimeItem = dec_row("f64", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_FLOAT_CMP: RuntimeItem = dec_row("float_cmp", RuntimeKind::Fn, DEC_GO_TS);
pub const DEC_FMT: RuntimeItem = dec_row("fmt", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_FMT_LIST: RuntimeItem = dec_row("fmt_list", RuntimeKind::Fn, DEC_RUST_TS);
pub const DEC_FMT_OPTION: RuntimeItem = dec_row("fmt_option", RuntimeKind::Fn, DEC_RUST);
/// `dec.fmt_option_with`: `fmt_option` printing the module's display tokens.
pub const DEC_FMT_OPTION_WITH: RuntimeItem = dec_row("fmt_option_with", RuntimeKind::Fn, DEC_RUST);
pub const DEC_INT: RuntimeItem = dec_row("int", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_MUL: RuntimeItem = dec_row("mul", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_NEG: RuntimeItem = dec_row("neg", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_OF: RuntimeItem = dec_row("of", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_PARSE: RuntimeItem = dec_row("parse", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_REM: RuntimeItem = dec_row("rem", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_RHE: RuntimeItem = dec_row("rhe", RuntimeKind::Fn, DEC_RUST);
pub const DEC_ROUND: RuntimeItem = dec_row("round", RuntimeKind::Fn, DEC_RUST);
pub const DEC_STORE: RuntimeItem = dec_row("store", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_SUB: RuntimeItem = dec_row("sub", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_TO_INT: RuntimeItem = dec_row("to_int", RuntimeKind::Fn, DEC_GO_TS);
pub const DEC_TRY_PARSE: RuntimeItem = dec_row("try_parse", RuntimeKind::Fn, DEC_RUST_GO_TS);
pub const DEC_TRY_STORE: RuntimeItem = dec_row("try_store", RuntimeKind::Fn, DEC_RUST_GO_TS);

/// The targets whose runtime package carries each `inf` row: the Rust runtime
/// (`faber::inf`), the Go runtime (`rt.Inf*`) and the TypeScript runtime
/// (`inf` in `@faber/runtime`, T1-T4) share the rows whose id and meaning
/// match.
const INF_RUST: &[Target] = &[Target::HirRust];
const INF_GO: &[Target] = &[Target::HirGo];
const INF_TS: &[Target] = &[Target::HirTypeScript];
const INF_RUST_GO: &[Target] = &[Target::HirRust, Target::HirGo];
const INF_RUST_TS: &[Target] = &[Target::HirRust, Target::HirTypeScript];
const INF_GO_TS: &[Target] = &[Target::HirTypeScript, Target::HirGo];
const INF_RUST_GO_TS: &[Target] = &[Target::HirRust, Target::HirTypeScript, Target::HirGo];

const fn inf_row(name: &'static str, kind: RuntimeKind, targets: &'static [Target]) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Inf,
        name,
        kind,
        targets,
    }
}

/// `inf` rows (T1-G4): the unbounded-integer (`*big.Int`) carrier of the Go
/// runtime (`rt.Inf*`), the rows above the Rust block. An `inf` is a `*big.Int` treated as immutable; a nil
/// pointer reads as zero through `INF_Z`. The width limit applies only in the
/// store family (`store_i`, `store_u`, `wrap`, `clamp_i`, `clamp_u`): a value
/// leaves the carrier into a bounded slot there and nowhere else. `INF_PART` is
/// the type an `inf` rides inside a composite (tuple) map key.
pub const INF_Z: RuntimeItem = inf_row("z", RuntimeKind::Fn, INF_GO);
pub const INF_I: RuntimeItem = inf_row("i", RuntimeKind::Fn, INF_GO);
pub const INF_U: RuntimeItem = inf_row("u", RuntimeKind::Fn, INF_GO);
pub const INF_S: RuntimeItem = inf_row("s", RuntimeKind::Fn, INF_GO);
pub const INF_SIGN: RuntimeItem = inf_row("sign", RuntimeKind::Fn, INF_GO);
pub const INF_ADD: RuntimeItem = inf_row("add", RuntimeKind::Fn, INF_GO);
pub const INF_SUB: RuntimeItem = inf_row("sub", RuntimeKind::Fn, INF_GO);
pub const INF_MUL: RuntimeItem = inf_row("mul", RuntimeKind::Fn, INF_GO);
pub const INF_NEG: RuntimeItem = inf_row("neg", RuntimeKind::Fn, INF_GO);
pub const INF_ABS: RuntimeItem = inf_row("abs", RuntimeKind::Fn, INF_GO_TS);
pub const INF_NOT: RuntimeItem = inf_row("not", RuntimeKind::Fn, INF_GO);
pub const INF_AND: RuntimeItem = inf_row("and", RuntimeKind::Fn, INF_GO);
pub const INF_OR: RuntimeItem = inf_row("or", RuntimeKind::Fn, INF_GO);
pub const INF_XOR: RuntimeItem = inf_row("xor", RuntimeKind::Fn, INF_GO);
pub const INF_DIV: RuntimeItem = inf_row("div", RuntimeKind::Fn, INF_RUST_GO_TS);
pub const INF_MOD: RuntimeItem = inf_row("mod", RuntimeKind::Fn, INF_GO_TS);
pub const INF_SHL: RuntimeItem = inf_row("shl", RuntimeKind::Fn, INF_RUST_GO_TS);
pub const INF_SHR: RuntimeItem = inf_row("shr", RuntimeKind::Fn, INF_RUST_GO_TS);
pub const INF_POW: RuntimeItem = inf_row("pow", RuntimeKind::Fn, INF_RUST_GO_TS);
pub const INF_SUM: RuntimeItem = inf_row("sum", RuntimeKind::Fn, INF_GO);
pub const INF_SIGNUM: RuntimeItem = inf_row("signum", RuntimeKind::Fn, INF_GO);
pub const INF_MAX: RuntimeItem = inf_row("max", RuntimeKind::Fn, INF_GO);
pub const INF_MIN: RuntimeItem = inf_row("min", RuntimeKind::Fn, INF_GO);
pub const INF_LIST_EQ: RuntimeItem = inf_row("list_eq", RuntimeKind::Fn, INF_GO);
pub const INF_CMP: RuntimeItem = inf_row("cmp", RuntimeKind::Fn, INF_GO);
pub const INF_LT_F: RuntimeItem = inf_row("lt_f", RuntimeKind::Fn, INF_GO);
pub const INF_LE_F: RuntimeItem = inf_row("le_f", RuntimeKind::Fn, INF_GO);
pub const INF_GT_F: RuntimeItem = inf_row("gt_f", RuntimeKind::Fn, INF_GO);
pub const INF_GE_F: RuntimeItem = inf_row("ge_f", RuntimeKind::Fn, INF_GO);
pub const INF_EQ_F: RuntimeItem = inf_row("eq_f", RuntimeKind::Fn, INF_GO);
pub const INF_APPROX: RuntimeItem = inf_row("approx", RuntimeKind::Fn, INF_RUST_GO_TS);
pub const INF_F64: RuntimeItem = inf_row("f64", RuntimeKind::Fn, INF_GO);
pub const INF_F32: RuntimeItem = inf_row("f32", RuntimeKind::Fn, INF_RUST_GO);
pub const INF_FROM_F: RuntimeItem = inf_row("from_f", RuntimeKind::Fn, INF_GO);
pub const INF_PARSE: RuntimeItem = inf_row("parse", RuntimeKind::Fn, INF_GO_TS);
pub const INF_STR: RuntimeItem = inf_row("str", RuntimeKind::Fn, INF_GO);
pub const INF_FROM_VALOR: RuntimeItem = inf_row("from_valor", RuntimeKind::Fn, INF_GO_TS);
pub const INF_FROM_BYTES: RuntimeItem = inf_row("from_bytes", RuntimeKind::Fn, INF_GO_TS);
pub const INF_TO_BYTES: RuntimeItem = inf_row("to_bytes", RuntimeKind::Fn, INF_GO_TS);
pub const INF_PACK: RuntimeItem = inf_row("pack", RuntimeKind::Fn, INF_RUST_GO_TS);
pub const INF_FITS_I: RuntimeItem = inf_row("fits_i", RuntimeKind::Fn, INF_GO);
pub const INF_FITS_U: RuntimeItem = inf_row("fits_u", RuntimeKind::Fn, INF_GO);
pub const INF_STORE_I: RuntimeItem = inf_row("store_i", RuntimeKind::StoreFn, INF_GO);
pub const INF_STORE_U: RuntimeItem = inf_row("store_u", RuntimeKind::StoreFn, INF_GO);
pub const INF_WRAP: RuntimeItem = inf_row("wrap", RuntimeKind::StoreFn, INF_GO);
pub const INF_CLAMP_I: RuntimeItem = inf_row("clamp_i", RuntimeKind::StoreFn, INF_GO);
pub const INF_CLAMP_U: RuntimeItem = inf_row("clamp_u", RuntimeKind::StoreFn, INF_GO);
pub const INF_TO_DEC: RuntimeItem = inf_row("to_dec", RuntimeKind::Fn, INF_RUST_GO_TS);
pub const INF_KEY: RuntimeItem = inf_row("key", RuntimeKind::Fn, INF_GO);
pub const INF_FROM_KEY: RuntimeItem = inf_row("from_key", RuntimeKind::Fn, INF_GO);
pub const INF_PART: RuntimeItem = inf_row("part", RuntimeKind::Type, INF_GO);

/// `inf` rows (T1-R3): the wrappers over `faber::Magnus` of the Rust runtime
/// (`faber::inf`), the traps and conversions its operators do not spell
/// themselves. `INF_STORE` is the store-family row of the Rust runtime: the
/// width limit of an `inf` landing in a bounded slot applies at a store site
/// and nowhere else.
pub const INF_BIG: RuntimeItem = inf_row("big", RuntimeKind::Fn, INF_RUST);
pub const INF_CMP_DEC: RuntimeItem = inf_row("cmp_dec", RuntimeKind::Fn, INF_RUST);
pub const INF_CMP_FLOAT: RuntimeItem = inf_row("cmp_float", RuntimeKind::Fn, INF_RUST_TS);
pub const INF_NARROW: RuntimeItem = inf_row("narrow", RuntimeKind::Fn, INF_RUST);
pub const INF_REM: RuntimeItem = inf_row("rem", RuntimeKind::Fn, INF_RUST);
pub const INF_STORE: RuntimeItem = inf_row("store", RuntimeKind::StoreFn, INF_RUST_TS);
pub const INF_TRUE_DIV: RuntimeItem = inf_row("true_div", RuntimeKind::Fn, INF_RUST_TS);

/// `inf` rows (T1-T4): the TypeScript runtime (`inf`) carries an `inf` as a
/// native `bigint`. The rows it shares with Rust and Go are marked above
/// (`div`, `mod`, `shl`, `shr`, `pow`, `abs`, `approx`, `pack`, `to_dec`,
/// `parse`, `from_valor`, `from_bytes`, `to_bytes`, `true_div`, `cmp_float`,
/// `store`); `to_number` and `clamp` exist only there. `to_number` is the
/// `number` image of a `bigint` (it traps past 2^53 - 1); `clamp` is the
/// saturating store, with `store`, the only place a width limit applies.
pub const INF_TO_NUMBER: RuntimeItem = inf_row("to_number", RuntimeKind::Fn, INF_TS);
pub const INF_CLAMP: RuntimeItem = inf_row("clamp", RuntimeKind::StoreFn, INF_TS);

/// `gen` rows (T1-R7): the lazy sync-generator carrier of the Rust runtime
/// (`faber::Gen`, `faber::Yield`, crate-root types per fork F-A; the module is
/// `faber::r#gen` because `gen` is reserved in edition 2024). A generator body
/// is an `async` block rustc lowers to a state machine; `cede` parks the item
/// through `Yield` and `Gen` is the iterator the consumer pulls.
pub const GEN_GEN: RuntimeItem = RuntimeItem {
    ns: Namespace::Gen,
    name: "Gen",
    kind: RuntimeKind::Type,
    targets: &[Target::HirRust],
};
pub const GEN_YIELD: RuntimeItem = RuntimeItem {
    ns: Namespace::Gen,
    name: "Yield",
    kind: RuntimeKind::Type,
    targets: &[Target::HirRust],
};

/// `gen` rows (T1-R8): the async-generator cursor (`fient`) and `block_on` of the
/// Rust runtime. The cursor types are crate-root (fork F-A) and `block_on` is a
/// free function of the `gen` module (`faber::r#gen::block_on`, the std form).
/// `block_on_tokio` is the Tokio form, behind the crate's optional `tokio`
/// feature; the emitter picks it by the library-binding fact it already had.
pub const GEN_ASYNC_CURSOR: RuntimeItem = RuntimeItem {
    ns: Namespace::Gen,
    name: "AsyncCursor",
    kind: RuntimeKind::Type,
    targets: &[Target::HirRust],
};
pub const GEN_ASYNC_CURSOR_SHARED: RuntimeItem = RuntimeItem {
    ns: Namespace::Gen,
    name: "AsyncCursorShared",
    kind: RuntimeKind::Type,
    targets: &[Target::HirRust],
};
pub const GEN_BLOCK_ON: RuntimeItem = RuntimeItem {
    ns: Namespace::Gen,
    name: "block_on",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirRust],
};
pub const GEN_BLOCK_ON_TOKIO: RuntimeItem = RuntimeItem {
    ns: Namespace::Gen,
    name: "block_on_tokio",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirRust],
};

/// `gen` row: answering a consumer request whose receiving end may have hung
/// up (`faber::r#gen::send_or_drop`). The answer is dropped when nobody waits
/// for it, so the generated producer never discards a `Result` itself.
pub const GEN_SEND_OR_DROP: RuntimeItem = RuntimeItem {
    ns: Namespace::Gen,
    name: "send_or_drop",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirRust],
};

/// `float` row (T1-R7, fork F-D): the half-float bit decode of the Rust runtime
/// (`faber::float::f16_bits_to_f32`). The Rust target has no std half-float
/// carrier, so `fractus<f16>` is a decoded-`f32` conversion intermediate.
pub const FLOAT_F16_BITS_TO_F32: RuntimeItem = RuntimeItem {
    ns: Namespace::Float,
    name: "f16_bits_to_f32",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirRust],
};

/// `float` row: the bfloat16 bit decode (`faber::float::bf16_bits_to_f32`),
/// the `b16 ↦ bf16` conversion on the Rust target.
pub const FLOAT_BF16_BITS_TO_F32: RuntimeItem = RuntimeItem {
    ns: Namespace::Float,
    name: "bf16_bits_to_f32",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirRust],
};

/// `float` row: the half-float bit encode (`faber::float::f32_to_f16_bits`),
/// the `f16 ↦ b16` conversion on the Rust target.
pub const FLOAT_F32_TO_F16_BITS: RuntimeItem = RuntimeItem {
    ns: Namespace::Float,
    name: "f32_to_f16_bits",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirRust],
};

/// `float` row: the bfloat16 bit encode (`faber::float::f32_to_bf16_bits`),
/// the `bf16 ↦ b16` conversion on the Rust target.
pub const FLOAT_F32_TO_BF16_BITS: RuntimeItem = RuntimeItem {
    ns: Namespace::Float,
    name: "f32_to_bf16_bits",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirRust],
};

/// `trap` rows (R0, `faber::trap`): the one place generated Rust fails. A
/// failing index, key, conversion or assertion calls a `trap` function with a
/// [`Trap`](`faber::Trap`) kind instead of writing `.expect(..)` / `panic!(..)`
/// inline. The functions unwind with the failure text the inline form printed;
/// `trap.exit_on_trap`, called first by the generated `main`, makes them print
/// the shared trap line and exit with the trap's sysexits class instead
/// (`faber::trap` module doc). `trap.Trap` is the closed kind enum, spelled at the crate root
/// as `faber::Trap`. No row applies a width limit.
const TRAP_RUST: &[Target] = &[Target::HirRust];

const fn trap_row(name: &'static str, kind: RuntimeKind) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Trap,
        name,
        kind,
        targets: TRAP_RUST,
    }
}

pub const TRAP_TRAP: RuntimeItem = trap_row("Trap", RuntimeKind::Type);
pub const TRAP_FAIL: RuntimeItem = trap_row("fail", RuntimeKind::Fn);
pub const TRAP_RAISE: RuntimeItem = trap_row("raise", RuntimeKind::Fn);
pub const TRAP_ABORT: RuntimeItem = trap_row("abort", RuntimeKind::Fn);
pub const TRAP_EXIT_ON_TRAP: RuntimeItem = trap_row("exit_on_trap", RuntimeKind::Fn);
pub const TRAP_SOME: RuntimeItem = trap_row("some", RuntimeKind::Fn);
pub const TRAP_OK: RuntimeItem = trap_row("ok", RuntimeKind::Fn);
pub const TRAP_OK_STATIC: RuntimeItem = trap_row("ok_static", RuntimeKind::Fn);
pub const TRAP_REQUIRE: RuntimeItem = trap_row("require", RuntimeKind::Fn);
pub const TRAP_UNREACHABLE: RuntimeItem = trap_row("unreachable", RuntimeKind::Fn);

/// TypeScript JSON, dynamic-value, variant, tuple-map and atomic carriers (T1-T11).
pub const JSON_VALUE: RuntimeItem = RuntimeItem {
    ns: Namespace::Json,
    name: "Value",
    kind: RuntimeKind::Type,
    targets: &[Target::HirTypeScript],
};
pub const JSON_PARSE: RuntimeItem = RuntimeItem {
    ns: Namespace::Json,
    name: "parse",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const JSON_STRINGIFY: RuntimeItem = RuntimeItem {
    ns: Namespace::Json,
    name: "stringify",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const JSON_PARSE_BIG_INT: RuntimeItem = RuntimeItem {
    ns: Namespace::Json,
    name: "parse_big_int",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const JSON_STRINGIFY_BIG_INT: RuntimeItem = RuntimeItem {
    ns: Namespace::Json,
    name: "stringify_big_int",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const VALOR_BOX: RuntimeItem = RuntimeItem {
    ns: Namespace::Valor,
    name: "box",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const VALOR_PAYLOAD: RuntimeItem = RuntimeItem {
    ns: Namespace::Valor,
    name: "payload",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const VALOR_IS: RuntimeItem = RuntimeItem {
    ns: Namespace::Valor,
    name: "is",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const VALOR_IS_INTEGER: RuntimeItem = RuntimeItem {
    ns: Namespace::Valor,
    name: "is_integer",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const VARIANT_VALUE: RuntimeItem = RuntimeItem {
    ns: Namespace::Variant,
    name: "value",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const TUPLE_MAP: RuntimeItem = RuntimeItem {
    ns: Namespace::Tuple,
    name: "TupleMap",
    kind: RuntimeKind::Type,
    targets: &[Target::HirTypeScript],
};
pub const ATOMIC_ATOMIC: RuntimeItem = RuntimeItem {
    ns: Namespace::Atomic,
    name: "Atomic",
    kind: RuntimeKind::Type,
    targets: &[Target::HirTypeScript],
};

/// The targets whose runtime package carries each `format` row: the Rust
/// runtime (`faber::format`), the Go runtime (`rt.Format*`) and the TypeScript
/// runtime (`format` in `@faber/runtime`, T1-T5) share the renderers whose id
/// and meaning match.
const FORMAT_RUST: &[Target] = &[Target::HirRust];
const FORMAT_GO: &[Target] = &[Target::HirGo];
const FORMAT_RUST_GO: &[Target] = &[Target::HirRust, Target::HirGo];
const FORMAT_RUST_GO_TS: &[Target] = &[Target::HirRust, Target::HirTypeScript, Target::HirGo];

const fn format_row(
    name: &'static str,
    kind: RuntimeKind,
    targets: &'static [Target],
) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Format,
        name,
        kind,
        targets,
    }
}

/// `format` rows (T1-R4, T1-G5): the `¶` format-operator renderers of the Rust
/// runtime (`faber::format`) and the Go runtime (`rt.Format*`). The decomposed
/// layout spec a call site builds as a struct literal is a type: `FORMAT_FMT`
/// (`faber::Fmt`, crate-root) in Rust and `FORMAT_SPEC` (`rt.FormatSpec`) in Go;
/// the rest are functions. `MAGNUS` and `MAGNUS_SCI` render an unbounded `inf`
/// (TypeScript renders a `bigint` through `int`, so it has no such rows). The
/// TypeScript runtime also declares a structural `format.Spec` type; generated
/// TypeScript passes an object literal and never names it, so it has no row.
/// Width limits do not apply here: a renderer lays out text, it never stores
/// into a bounded cell.
pub const FORMAT_FMT: RuntimeItem = format_row("Fmt", RuntimeKind::Type, FORMAT_RUST);
pub const FORMAT_SPEC: RuntimeItem = format_row("Spec", RuntimeKind::Type, FORMAT_GO);
pub const FORMAT_DECIMAL: RuntimeItem = format_row("decimal", RuntimeKind::Fn, FORMAT_RUST_GO);
pub const FORMAT_FLOAT: RuntimeItem = format_row("float", RuntimeKind::Fn, FORMAT_RUST_GO_TS);
pub const FORMAT_INSTANS: RuntimeItem = format_row("instans", RuntimeKind::Fn, FORMAT_RUST_GO_TS);
pub const FORMAT_INT: RuntimeItem = format_row("int", RuntimeKind::Fn, FORMAT_RUST_GO_TS);
pub const FORMAT_MAGNUS: RuntimeItem = format_row("magnus", RuntimeKind::Fn, FORMAT_RUST_GO);
pub const FORMAT_MAGNUS_SCI: RuntimeItem =
    format_row("magnus_sci", RuntimeKind::Fn, FORMAT_RUST_GO);
pub const FORMAT_PAD: RuntimeItem = format_row("pad", RuntimeKind::Fn, FORMAT_RUST_GO_TS);
pub const FORMAT_SCI: RuntimeItem = format_row("sci", RuntimeKind::Fn, FORMAT_RUST_GO_TS);
pub const FORMAT_SIGNED: RuntimeItem = format_row("signed", RuntimeKind::Fn, FORMAT_GO);
pub const FORMAT_TEXT: RuntimeItem = format_row("text", RuntimeKind::Fn, FORMAT_RUST_GO_TS);

/// The targets whose runtime package carries a `tensor`, `vector` or `sparsa`
/// row. Go carries its flat carrier and free-function rows; TypeScript shares
/// the tensor and sparse types and carries matrix and normalization rows.
const SHAPED_GO: &[Target] = &[Target::HirGo];

const fn shaped_row(ns: Namespace, name: &'static str, kind: RuntimeKind) -> RuntimeItem {
    RuntimeItem {
        ns,
        name,
        kind,
        targets: SHAPED_GO,
    }
}

/// `tensor`, `vector` and `sparsa` rows (T1-G7): the shaped-numeric carriers of
/// the Go runtime. A carrier is a type; its operations (`Addita`, `Matmul`,
/// `Productum`, `Densata`, ...) are methods of that type, carrying the Latin
/// method names the call sites already use, so they are not table rows. The
/// rows are the three carrier types plus the four free functions a call site
/// names: the element count a shape implies (checked before building a recovery
/// tensor), the dense-to-sparse conversion, the width-checked vector
/// constructor and the sparse vacua constructor. No row applies a width limit:
/// a carrier computes in its element type and never stores into a bounded cell.
pub const TENSOR_TENSOR: RuntimeItem = RuntimeItem {
    targets: &[Target::HirGo, Target::HirTypeScript],
    ..shaped_row(Namespace::Tensor, "Tensor", RuntimeKind::Type)
};
pub const TENSOR_ELEMENT_COUNT: RuntimeItem =
    shaped_row(Namespace::Tensor, "element_count", RuntimeKind::Fn);
pub const TENSOR_TO_SPARSA: RuntimeItem =
    shaped_row(Namespace::Tensor, "to_sparsa", RuntimeKind::Fn);
pub const VECTOR_VECTOR: RuntimeItem = shaped_row(Namespace::Vector, "Vector", RuntimeKind::Type);
pub const VECTOR_FROM_LIST: RuntimeItem =
    shaped_row(Namespace::Vector, "from_list", RuntimeKind::Fn);
pub const SPARSA_SPARSA: RuntimeItem = RuntimeItem {
    targets: &[Target::HirGo, Target::HirTypeScript],
    ..shaped_row(Namespace::Sparsa, "Sparsa", RuntimeKind::Type)
};
/// TypeScript shaped-carrier rows. Operations remain methods of the carriers;
/// normalization is a free function rather than an Array prototype extension.
pub const MATRIX_MATRIX: RuntimeItem = RuntimeItem {
    ns: Namespace::Matrix,
    name: "Matrix",
    kind: RuntimeKind::Type,
    targets: &[Target::HirTypeScript],
};
pub const VECTOR_NORMALIZE: RuntimeItem = RuntimeItem {
    ns: Namespace::Vector,
    name: "normalize",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const SPARSA_NEW: RuntimeItem = shaped_row(Namespace::Sparsa, "new", RuntimeKind::Fn);

/// The targets whose runtime package carries an `order`, `intervallum`, `tuple`,
/// `instans` or `valor` row. The Go runtime (`rt.Order*`, `rt.Intervallum*`,
/// `rt.Tuple*`, `rt.Instans*`, `rt.Valor*`,
/// `faber/runtime/go/{order,intervallum,tuple,instans,valor}.go`) carries them;
/// the TypeScript runtime shares `order.compare` (T1-T13).
const CARRIER_GO: &[Target] = &[Target::HirGo];

/// The `order` and `dup` rows shared with Go (`order.compare`) or carried by
/// the TypeScript runtime alone (`order.textCompare`, `dup.duplicate`).
const CARRIER_GO_TS: &[Target] = &[Target::HirGo, Target::HirTypeScript];
const CARRIER_TS: &[Target] = &[Target::HirTypeScript];

const fn carrier_row(ns: Namespace, name: &'static str, kind: RuntimeKind) -> RuntimeItem {
    RuntimeItem {
        ns,
        name,
        kind,
        targets: CARRIER_GO,
    }
}

/// `order`, `intervallum`, `tuple`, `instans` and `valor` rows (T1-G8): the Go
/// carriers and decoders a call site names. `order` is the generic
/// ordering-contract entry (a genus's own `Compare`, else the built-in
/// compare) and `compare` the built-in and tuple three-way compare. The
/// interval carrier is a type whose operations (`Continet`, `Coercere`,
/// `Inter`, ...) are methods carrying the Latin names the call sites use; its
/// constructor is the one free function a call site names. `tuple` turns a
/// tuple carrier into a comparable map key and back. `instans.parse` is the
/// three-layout time parse, `valor.struct` the valor-to-genus decoder shell
/// whose field extraction is supplied at the call site. No row applies a width
/// limit: none of them stores into a bounded cell.
pub const ORDER_ORDER: RuntimeItem = carrier_row(Namespace::Order, "order", RuntimeKind::Fn);
/// `order.compare` (T1-G8, T1-T13): the built-in and tuple three-way compare.
/// The TypeScript runtime shares the row: its `order.compare` helper orders
/// built-ins, tuples and a genus's own `compare`, so it carries the same id.
pub const ORDER_COMPARE: RuntimeItem = RuntimeItem {
    ns: Namespace::Order,
    name: "compare",
    kind: RuntimeKind::Fn,
    targets: CARRIER_GO_TS,
};
/// `order.float_less`: whether one float sorts before another in the total
/// order (NaN last, `-0` before `+0`), the test a sort of floats uses.
pub const ORDER_FLOAT_LESS: RuntimeItem =
    carrier_row(Namespace::Order, "float_less", RuntimeKind::Fn);
/// `order.equal`: the structural `≡` / `≢` over a genus (a Go `*Name` compares
/// by identity under `==`), a list, map, set, tuple or optional of one.
pub const ORDER_EQUAL: RuntimeItem = carrier_row(Namespace::Order, "equal", RuntimeKind::Fn);
/// `order.text_compare` (T1-T13): code-point text compare of the TypeScript
/// runtime (`order.textCompare`). Go compares strings by UTF-8 byte order,
/// which equals code-point order, so it needs no such helper.
pub const ORDER_TEXT_COMPARE: RuntimeItem = RuntimeItem {
    ns: Namespace::Order,
    name: "text_compare",
    kind: RuntimeKind::Fn,
    targets: CARRIER_TS,
};
/// `dup.duplicate` (T1-T13): the deep duplicate of the TypeScript runtime
/// (`dup.duplicate`), called at a `copy` bind, a copy parameter and the sermo
/// boundary. Only the TypeScript runtime carries it.
pub const DUP_DUPLICATE: RuntimeItem = RuntimeItem {
    ns: Namespace::Dup,
    name: "duplicate",
    kind: RuntimeKind::Fn,
    targets: CARRIER_TS,
};
pub const INTERVALLUM_INTERVALLUM: RuntimeItem =
    carrier_row(Namespace::Intervallum, "Intervallum", RuntimeKind::Type);
pub const INTERVALLUM_VALUE: RuntimeItem =
    carrier_row(Namespace::Intervallum, "value", RuntimeKind::Fn);
/// `tuple.genus_key`: Go genus pointers are identity-comparable, but Faber genus
/// keys compare by their hashable field values.
pub const TUPLE_GENUS_KEY: RuntimeItem =
    carrier_row(Namespace::Tuple, "genus_key", RuntimeKind::Fn);
pub const TUPLE_KEY: RuntimeItem = carrier_row(Namespace::Tuple, "key", RuntimeKind::Fn);
/// `tuple.parts`: decode a Go composite key back to its tuple carrier.
pub const TUPLE_PARTS: RuntimeItem = carrier_row(Namespace::Tuple, "parts", RuntimeKind::Fn);
pub const INSTANS_PARSE: RuntimeItem = carrier_row(Namespace::Instans, "parse", RuntimeKind::Fn);
pub const VALOR_STRUCT: RuntimeItem = carrier_row(Namespace::Valor, "struct", RuntimeKind::Fn);

/// The shared conversation type carriers live in Go and TypeScript. Go's
/// variant types and free functions remain Go-only rows.
const SERMO_GO_TS: &[Target] = &[Target::HirGo, Target::HirTypeScript];

const SERMO_GO: &[Target] = &[Target::HirGo];

const fn sermo_row(name: &'static str, kind: RuntimeKind) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Sermo,
        name,
        kind,
        targets: SERMO_GO,
    }
}

/// `sermo` rows (T1-G9): the frame conversation runtime of the Go target. The
/// types are the sermo handle and its directional views, the typed frame
/// (`scrinium<T>`), the `status` enum with one struct per variant, and the
/// conversation record a served `@ ad` handler receives. The functions open and
/// dispatch a conversation, install the program's static route table, push an
/// item frame from a handler, build the directional views and materialize a
/// sermo into a text, numeric or vacuum value. The frame family has no
/// namespace of its own: it rides the `sermo` namespace. No row applies a width
/// limit.
pub const SERMO_SERMO: RuntimeItem = RuntimeItem {
    ns: Namespace::Sermo,
    name: "Sermo",
    kind: RuntimeKind::Type,
    targets: SERMO_GO_TS,
};
pub const SERMO_MEUS: RuntimeItem = RuntimeItem {
    ns: Namespace::Sermo,
    name: "Meus",
    kind: RuntimeKind::Type,
    targets: SERMO_GO_TS,
};
pub const SERMO_TUUS: RuntimeItem = RuntimeItem {
    ns: Namespace::Sermo,
    name: "Tuus",
    kind: RuntimeKind::Type,
    targets: SERMO_GO_TS,
};
pub const SERMO_SCRINIUM: RuntimeItem = RuntimeItem {
    ns: Namespace::Sermo,
    name: "Scrinium",
    kind: RuntimeKind::Type,
    targets: SERMO_GO_TS,
};
pub const SERMO_STATUS: RuntimeItem = RuntimeItem {
    ns: Namespace::Sermo,
    name: "Status",
    kind: RuntimeKind::Type,
    targets: SERMO_GO_TS,
};
pub const SERMO_REQUEST: RuntimeItem = sermo_row("Request", RuntimeKind::Type);
pub const SERMO_ITEM: RuntimeItem = sermo_row("Item", RuntimeKind::Type);
pub const SERMO_BYTE: RuntimeItem = sermo_row("Byte", RuntimeKind::Type);
pub const SERMO_BULK: RuntimeItem = sermo_row("Bulk", RuntimeKind::Type);
pub const SERMO_DONE: RuntimeItem = sermo_row("Done", RuntimeKind::Type);
pub const SERMO_ERROR: RuntimeItem = sermo_row("Error", RuntimeKind::Type);
pub const SERMO_CANCEL: RuntimeItem = sermo_row("Cancel", RuntimeKind::Type);
pub const SERMO_CONVERSATION: RuntimeItem = sermo_row("Conversation", RuntimeKind::Type);
pub const SERMO_CONV_ITEM: RuntimeItem = sermo_row("conv_item", RuntimeKind::Fn);
pub const SERMO_OPEN: RuntimeItem = sermo_row("open", RuntimeKind::Fn);
pub const SERMO_SET_OPENER: RuntimeItem = sermo_row("set_opener", RuntimeKind::Fn);
pub const SERMO_DISPATCH: RuntimeItem = sermo_row("dispatch", RuntimeKind::Fn);
pub const SERMO_INSTALL_ROUTES: RuntimeItem = sermo_row("install_routes", RuntimeKind::Fn);
pub const SERMO_AS_MEUS: RuntimeItem = sermo_row("as_meus", RuntimeKind::Fn);
pub const SERMO_AS_TUUS: RuntimeItem = sermo_row("as_tuus", RuntimeKind::Fn);
pub const SERMO_READ_TEXT: RuntimeItem = sermo_row("read_text", RuntimeKind::Fn);
/// `sermo.read_text_result`: the non-trapping text reader, `(string, error)`; a
/// terminal `error` frame is a `*FrameFault` (Go).
pub const SERMO_READ_TEXT_RESULT: RuntimeItem = sermo_row("read_text_result", RuntimeKind::Fn);
/// `sermo.fault_record`: the `{kind, path}` crossing record of a failed
/// materialization (Go and TypeScript).
pub const SERMO_FAULT_RECORD: RuntimeItem = RuntimeItem {
    ns: Namespace::Sermo,
    name: "fault_record",
    kind: RuntimeKind::Fn,
    targets: SERMO_GO_TS,
};
pub const SERMO_READ_NUMERUS: RuntimeItem = sermo_row("read_numerus", RuntimeKind::Fn);
pub const SERMO_DRAIN: RuntimeItem = sermo_row("drain", RuntimeKind::Fn);
/// Typed conversation readers (Go): the trapping form and the `_result` form
/// whose failure is the returned error (a `*FrameFault` for a terminal `error`
/// frame), for a `valor`, bool, float, byte buffer and list of text.
pub const SERMO_READ_VALUE: RuntimeItem = sermo_row("read_value", RuntimeKind::Fn);
pub const SERMO_READ_VALUE_RESULT: RuntimeItem = sermo_row("read_value_result", RuntimeKind::Fn);
pub const SERMO_READ_BOOL: RuntimeItem = sermo_row("read_bool", RuntimeKind::Fn);
pub const SERMO_READ_BOOL_RESULT: RuntimeItem = sermo_row("read_bool_result", RuntimeKind::Fn);
pub const SERMO_READ_FLOAT: RuntimeItem = sermo_row("read_float", RuntimeKind::Fn);
pub const SERMO_READ_FLOAT_RESULT: RuntimeItem = sermo_row("read_float_result", RuntimeKind::Fn);
pub const SERMO_READ_BYTES: RuntimeItem = sermo_row("read_bytes", RuntimeKind::Fn);
pub const SERMO_READ_BYTES_RESULT: RuntimeItem = sermo_row("read_bytes_result", RuntimeKind::Fn);
pub const SERMO_READ_STRINGS: RuntimeItem = sermo_row("read_strings", RuntimeKind::Fn);
pub const SERMO_READ_STRINGS_RESULT: RuntimeItem =
    sermo_row("read_strings_result", RuntimeKind::Fn);
/// `sermo.read_as`: the reader chosen by a type parameter (Go generic).
pub const SERMO_READ_AS: RuntimeItem = sermo_row("read_as", RuntimeKind::Fn);
pub const SERMO_READ_AS_RESULT: RuntimeItem = sermo_row("read_as_result", RuntimeKind::Fn);
/// `sermo.drain_result`: the unit drain whose failure is the returned error (Go).
pub const SERMO_DRAIN_RESULT: RuntimeItem = sermo_row("drain_result", RuntimeKind::Fn);
/// `sermo.read_line`: one process-wide stdin line as `string ∪ none` (Go).
pub const SERMO_READ_LINE: RuntimeItem = sermo_row("read_line", RuntimeKind::Fn);

/// `trap.text_window`: `text[start‥end]` by Unicode character, dying outside
/// `0‥len` like the MIR runner (Go; the Rust runtime has no such helper).
pub const TRAP_TEXT_WINDOW: RuntimeItem = RuntimeItem {
    ns: Namespace::Trap,
    name: "text_window",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirGo],
};

/// `text` rows (Go): the text operations whose Go standard-library spelling
/// differs from the MIR runner's, so they are runtime helpers. `split` keeps the
/// empty pieces around an empty separator; `parse_float` takes the runner's
/// float grammar and `float` renders a float as plain digits; `upper` and `lower` apply the full
/// Unicode case mapping (`ß` to `SS`, the final sigma).
const fn text_row(name: &'static str) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Text,
        name,
        kind: RuntimeKind::Fn,
        targets: &[Target::HirGo],
    }
}

pub const TEXT_SPLIT: RuntimeItem = text_row("split");
pub const TEXT_UPPER: RuntimeItem = text_row("upper");
pub const TEXT_LOWER: RuntimeItem = text_row("lower");
pub const TEXT_PARSE_FLOAT: RuntimeItem = text_row("parse_float");
pub const TEXT_FLOAT: RuntimeItem = text_row("float");

/// The property a thrown TypeScript assertion failure carries. It is a marker
/// on the thrown `Error`, not a runtime helper, so it has no table row; the
/// failable conversion wrapper reads it to rethrow a program defect instead of
/// folding it into a conversion failure.
pub const TS_ASSERTION_MARKER: &str = "__faberAssertion";

/// The targets whose runtime package carries each `regex` row. Rows differ by
/// target: the Rust runtime carries the match operations as free functions in
/// `faber::regex`, the Go runtime carries a constructor plus methods, the Swift
/// runtime a `Regex` namespace, so each row lists exactly the targets that
/// consume it.
const REGEX_RUST_SWIFT_PY: &[Target] = &[Target::HirRust, Target::HirSwift, Target::HirPython];
const REGEX_RUST_PY: &[Target] = &[Target::HirRust, Target::HirPython];
const REGEX_GO: &[Target] = &[Target::HirGo];
const REGEX_GO_SWIFT: &[Target] = &[Target::HirGo, Target::HirSwift];
const REGEX_TS: &[Target] = &[Target::HirTypeScript];
const REGEX_SWIFT_PY: &[Target] = &[Target::HirSwift, Target::HirPython];
const REGEX_GO_TS_SWIFT_PY: &[Target] = &[
    Target::HirGo,
    Target::HirTypeScript,
    Target::HirSwift,
    Target::HirPython,
];
const REGEX_ALL: &[Target] = &[
    Target::HirRust,
    Target::HirGo,
    Target::HirTypeScript,
    Target::HirSwift,
    Target::HirPython,
];

const fn regex_row(
    name: &'static str,
    kind: RuntimeKind,
    targets: &'static [Target],
) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Regex,
        name,
        kind,
        targets,
    }
}

/// `regex` rows (RX-7): the operations on a compiled `regex` and on a match,
/// as the Rust runtime carries them (`faber::regex`). A compiled pattern and a
/// match are the `faber::Regex` / `faber::Match` carriers; every operation is
/// total because the conversion `↦ regex` is the one place a pattern is
/// compiled or rejected. Offsets are code points.
pub const REGEX_MATCHES: RuntimeItem = regex_row("matches", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_FIND: RuntimeItem = regex_row("find", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_FIND_ALL: RuntimeItem = regex_row("find_all", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_SPLIT: RuntimeItem = regex_row("split", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_REPLACE: RuntimeItem = regex_row("replace", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_REPLACE_WITH: RuntimeItem =
    regex_row("replace_with", RuntimeKind::Fn, REGEX_RUST_PY);
pub const REGEX_MATCH_TEXT: RuntimeItem =
    regex_row("match_text", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_MATCH_START: RuntimeItem =
    regex_row("match_start", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_MATCH_END: RuntimeItem =
    regex_row("match_end", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_MATCH_GROUP: RuntimeItem =
    regex_row("match_group", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_MATCH_NAMED: RuntimeItem =
    regex_row("match_named", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);

/// `regex` rows (RX-9): the Go carrier of the RE2-dialect regex. `Regex` is the
/// compiled value, `Match` one successful match; the operations on them are
/// methods of those types, so only the constructor and the two helpers with no
/// receiver of that type are functions.
pub const REGEX_REGEX: RuntimeItem = regex_row("Regex", RuntimeKind::Type, REGEX_GO_TS_SWIFT_PY);
pub const REGEX_MATCH: RuntimeItem = regex_row("Match", RuntimeKind::Type, REGEX_GO_TS_SWIFT_PY);
pub const REGEX_NEW: RuntimeItem = regex_row("new", RuntimeKind::Fn, REGEX_GO);
pub const REGEX_ESCAPE: RuntimeItem = regex_row("escape", RuntimeKind::Fn, REGEX_ALL);
pub const REGEX_REPLACE_EACH: RuntimeItem =
    regex_row("replace_each", RuntimeKind::Fn, REGEX_GO_SWIFT);

/// `regex` rows (RX-8): the TypeScript runtime package's `regex` namespace
/// (`@faber/runtime/regex`). `Regex` is the compiled pattern, `Match` one
/// successful match (it has no source-level type name, RD-3), `RegexError` the
/// rejection of a pattern a conversion throws, and `escape` the `textus`
/// method `munita`. The operations are members of the `Regex` and `Match`
/// types, not table rows.
pub const REGEX_ERROR: RuntimeItem = regex_row("RegexError", RuntimeKind::Type, REGEX_TS);

/// `regex` rows (RX-10a): the Swift runtime (`Regex.Regex`, `Regex.Match`,
/// `Regex.compile`, ...). Swift consumes the same operation rows as the Rust
/// target (as free functions of the `Regex` namespace) plus the carrier types,
/// `replace_each` and `escape`; `compile` is its constructor.
pub const REGEX_COMPILE: RuntimeItem = regex_row("compile", RuntimeKind::Fn, REGEX_SWIFT_PY);

/// `regex` rows: the Rust constructors that compile a pattern and trap when it
/// is rejected (`faber::regex::compile_or_trap` for an unrecovered
/// `textus ↦ regex` conversion, `faber::regex::literal_or_trap` for a regex
/// literal the compiler wrote), so generated code never unwraps `Regex::new`.
pub const REGEX_COMPILE_OR_TRAP: RuntimeItem =
    regex_row("compile_or_trap", RuntimeKind::Fn, &[Target::HirRust]);
pub const REGEX_LITERAL_OR_TRAP: RuntimeItem =
    regex_row("literal_or_trap", RuntimeKind::Fn, &[Target::HirRust]);

/// Shared TypeScript route registry accessor and typed entries.
pub const ROUTES_TABLE: RuntimeItem = RuntimeItem {
    ns: Namespace::Routes,
    name: "table",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirTypeScript],
};
pub const ROUTES_HANDLER: RuntimeItem = RuntimeItem {
    ns: Namespace::Routes,
    name: "Handler",
    kind: RuntimeKind::Type,
    targets: &[Target::HirTypeScript],
};
pub const ROUTES_REGISTRY: RuntimeItem = RuntimeItem {
    ns: Namespace::Routes,
    name: "Registry",
    kind: RuntimeKind::Type,
    targets: &[Target::HirTypeScript],
};

/// The canonical helper table: a row for every helper some runtime package
/// carries. Each namespace unit adds its rows with the runtime it moves.
pub const TABLE: &[RuntimeItem] = &[
    ROUTES_TABLE,
    ROUTES_HANDLER,
    ROUTES_REGISTRY,
    modulus_ts_row("add_u8"),
    modulus_ts_row("sub_u8"),
    modulus_ts_row("mul_u8"),
    modulus_ts_row("and_u8"),
    modulus_ts_row("or_u8"),
    modulus_ts_row("xor_u8"),
    modulus_ts_row("neg_u8"),
    modulus_ts_row("bit_not_u8"),
    modulus_ts_row("div_u8"),
    modulus_ts_row("mod_u8"),
    modulus_ts_row("shl_u8"),
    modulus_ts_row("shr_u8"),
    modulus_ts_row("add_u16"),
    modulus_ts_row("sub_u16"),
    modulus_ts_row("mul_u16"),
    modulus_ts_row("and_u16"),
    modulus_ts_row("or_u16"),
    modulus_ts_row("xor_u16"),
    modulus_ts_row("neg_u16"),
    modulus_ts_row("bit_not_u16"),
    modulus_ts_row("div_u16"),
    modulus_ts_row("mod_u16"),
    modulus_ts_row("shl_u16"),
    modulus_ts_row("shr_u16"),
    modulus_ts_row("add_u32"),
    modulus_ts_row("sub_u32"),
    modulus_ts_row("mul_u32"),
    modulus_ts_row("and_u32"),
    modulus_ts_row("or_u32"),
    modulus_ts_row("xor_u32"),
    modulus_ts_row("neg_u32"),
    modulus_ts_row("bit_not_u32"),
    modulus_ts_row("div_u32"),
    modulus_ts_row("mod_u32"),
    modulus_ts_row("shl_u32"),
    modulus_ts_row("shr_u32"),
    modulus_ts_row("add_u64"),
    modulus_ts_row("sub_u64"),
    modulus_ts_row("mul_u64"),
    modulus_ts_row("and_u64"),
    modulus_ts_row("or_u64"),
    modulus_ts_row("xor_u64"),
    modulus_ts_row("neg_u64"),
    modulus_ts_row("bit_not_u64"),
    modulus_ts_row("div_u64"),
    modulus_ts_row("mod_u64"),
    modulus_ts_row("shl_u64"),
    modulus_ts_row("shr_u64"),
    modulus_ts_row("add_i8"),
    modulus_ts_row("sub_i8"),
    modulus_ts_row("mul_i8"),
    modulus_ts_row("and_i8"),
    modulus_ts_row("or_i8"),
    modulus_ts_row("xor_i8"),
    modulus_ts_row("neg_i8"),
    modulus_ts_row("bit_not_i8"),
    modulus_ts_row("div_i8"),
    modulus_ts_row("mod_i8"),
    modulus_ts_row("shl_i8"),
    modulus_ts_row("shr_i8"),
    modulus_ts_row("add_i16"),
    modulus_ts_row("sub_i16"),
    modulus_ts_row("mul_i16"),
    modulus_ts_row("and_i16"),
    modulus_ts_row("or_i16"),
    modulus_ts_row("xor_i16"),
    modulus_ts_row("neg_i16"),
    modulus_ts_row("bit_not_i16"),
    modulus_ts_row("div_i16"),
    modulus_ts_row("mod_i16"),
    modulus_ts_row("shl_i16"),
    modulus_ts_row("shr_i16"),
    modulus_ts_row("add_i32"),
    modulus_ts_row("sub_i32"),
    modulus_ts_row("mul_i32"),
    modulus_ts_row("and_i32"),
    modulus_ts_row("or_i32"),
    modulus_ts_row("xor_i32"),
    modulus_ts_row("neg_i32"),
    modulus_ts_row("bit_not_i32"),
    modulus_ts_row("div_i32"),
    modulus_ts_row("mod_i32"),
    modulus_ts_row("shl_i32"),
    modulus_ts_row("shr_i32"),
    modulus_ts_row("add_i64"),
    modulus_ts_row("sub_i64"),
    modulus_ts_row("mul_i64"),
    modulus_ts_row("and_i64"),
    modulus_ts_row("or_i64"),
    modulus_ts_row("xor_i64"),
    modulus_ts_row("neg_i64"),
    modulus_ts_row("bit_not_i64"),
    modulus_ts_row("div_i64"),
    modulus_ts_row("mod_i64"),
    modulus_ts_row("shl_i64"),
    modulus_ts_row("shr_i64"),
    DEC_CARRIER,
    DEC_ADD,
    DEC_CMP,
    DEC_DIV,
    DEC_F64,
    DEC_FLOAT_CMP,
    DEC_FMT,
    DEC_FMT_LIST,
    DEC_FMT_OPTION,
    DEC_FMT_OPTION_WITH,
    DEC_INT,
    DEC_MUL,
    DEC_NEG,
    DEC_OF,
    DEC_PARSE,
    DEC_REM,
    DEC_RHE,
    DEC_ROUND,
    DEC_STORE,
    DEC_SUB,
    DEC_TO_INT,
    DEC_TRY_PARSE,
    DEC_TRY_STORE,
    DISPLAY_BIVALENS,
    DISPLAY_BIVALENS_WITH,
    DISPLAY_DISPLAY_TOKENS,
    DISPLAY_FRACTUS,
    DISPLAY_LIST,
    DISPLAY_MAP,
    DISPLAY_NUMERUS,
    DISPLAY_OPTION,
    DISPLAY_OPTION_BIVALENS,
    DISPLAY_OPTION_BIVALENS_WITH,
    DISPLAY_OPTION_FRACTUS,
    DISPLAY_OPTION_FRACTUS_WITH,
    DISPLAY_OPTION_VACUUM,
    DISPLAY_OPTION_VACUUM_WITH,
    DISPLAY_OPTION_WITH,
    DISPLAY_SPARSA,
    DISPLAY_TAGGED_VALOR,
    DISPLAY_TENSOR,
    DISPLAY_TEXT_PAYLOAD,
    DISPLAY_VALOR,
    DISPLAY_VALOR_WITH,
    DISPLAY_VALUE,
    DISPLAY_VERUM,
    EXACT_ADD,
    EXACT_APPROX_INT,
    EXACT_APPROX_F32,
    EXACT_APPROX_F64,
    EXACT_CMP_INT_FLOAT,
    EXACT_CMP_SCALED_FLOAT,
    EXACT_DIV,
    EXACT_FIT,
    EXACT_INT,
    EXACT_MUL,
    EXACT_POW,
    EXACT_REM,
    EXACT_SHL,
    EXACT_SHR,
    EXACT_STORE,
    EXACT_X,
    EXACT_MK,
    EXACT_I,
    EXACT_U,
    EXACT_NEG,
    EXACT_ADD_S,
    EXACT_MOD,
    EXACT_W,
    EXACT_FROM_W,
    EXACT_AND,
    EXACT_OR,
    EXACT_XOR,
    EXACT_NOT,
    EXACT_ABS,
    EXACT_CMP,
    EXACT_APPROX,
    EXACT_CMP_F,
    EXACT_LT_F,
    EXACT_LE_F,
    EXACT_GT_F,
    EXACT_GE_F,
    EXACT_EQ_F,
    EXACT_STR,
    EXACT_TRAP,
    EXACT_FLOAT,
    EXACT_FITS_I,
    EXACT_FITS_U,
    EXACT_WRAP,
    EXACT_CLAMP_I,
    EXACT_CLAMP_U,
    EXACT_STORE_INFERRED,
    EXACT_SUB,
    EXACT_CHK,
    EXACT_HI,
    EXACT_LO,
    EXACT_JOIN,
    EXACT_FLOAT_REM,
    EXACT_TRUE_DIV,
    EXACT_TRUE_DIV_F32,
    EXACT_CLAMP,
    INF_Z,
    INF_I,
    INF_U,
    INF_S,
    INF_SIGN,
    INF_ADD,
    INF_SUB,
    INF_MUL,
    INF_NEG,
    INF_ABS,
    INF_NOT,
    INF_AND,
    INF_OR,
    INF_XOR,
    INF_DIV,
    INF_MOD,
    INF_SHL,
    INF_SHR,
    INF_POW,
    INF_SUM,
    INF_SIGNUM,
    INF_MAX,
    INF_MIN,
    INF_LIST_EQ,
    INF_CMP,
    INF_LT_F,
    INF_LE_F,
    INF_GT_F,
    INF_GE_F,
    INF_EQ_F,
    INF_APPROX,
    INF_F64,
    INF_F32,
    INF_FROM_F,
    INF_PARSE,
    INF_STR,
    INF_FROM_VALOR,
    INF_FROM_BYTES,
    INF_TO_BYTES,
    INF_PACK,
    INF_FITS_I,
    INF_FITS_U,
    INF_STORE_I,
    INF_STORE_U,
    INF_WRAP,
    INF_CLAMP_I,
    INF_CLAMP_U,
    INF_TO_DEC,
    INF_KEY,
    INF_FROM_KEY,
    INF_PART,
    INF_BIG,
    INF_CMP_DEC,
    INF_CMP_FLOAT,
    INF_NARROW,
    INF_REM,
    INF_STORE,
    INF_TRUE_DIV,
    GEN_GEN,
    GEN_YIELD,
    GEN_ASYNC_CURSOR,
    GEN_ASYNC_CURSOR_SHARED,
    GEN_BLOCK_ON,
    GEN_BLOCK_ON_TOKIO,
    GEN_SEND_OR_DROP,
    FLOAT_F16_BITS_TO_F32,
    FLOAT_BF16_BITS_TO_F32,
    FLOAT_F32_TO_F16_BITS,
    FLOAT_F32_TO_BF16_BITS,
    TRAP_TRAP,
    TRAP_FAIL,
    TRAP_ABORT,
    TRAP_EXIT_ON_TRAP,
    TRAP_RAISE,
    TRAP_SOME,
    TRAP_OK,
    TRAP_OK_STATIC,
    TRAP_REQUIRE,
    TRAP_UNREACHABLE,
    INF_TO_NUMBER,
    INF_CLAMP,
    FORMAT_FMT,
    FORMAT_SPEC,
    FORMAT_DECIMAL,
    FORMAT_FLOAT,
    FORMAT_INSTANS,
    FORMAT_INT,
    FORMAT_MAGNUS,
    FORMAT_MAGNUS_SCI,
    FORMAT_PAD,
    FORMAT_SCI,
    FORMAT_SIGNED,
    FORMAT_TEXT,
    JSON_VALUE,
    JSON_PARSE,
    JSON_STRINGIFY,
    JSON_PARSE_BIG_INT,
    JSON_STRINGIFY_BIG_INT,
    VALOR_BOX,
    VALOR_PAYLOAD,
    VALOR_IS,
    VALOR_IS_INTEGER,
    VARIANT_VALUE,
    TUPLE_MAP,
    ATOMIC_ATOMIC,
    TENSOR_TENSOR,
    MATRIX_MATRIX,
    VECTOR_NORMALIZE,
    TENSOR_ELEMENT_COUNT,
    TENSOR_TO_SPARSA,
    VECTOR_VECTOR,
    VECTOR_FROM_LIST,
    SPARSA_SPARSA,
    SPARSA_NEW,
    ORDER_ORDER,
    ORDER_COMPARE,
    ORDER_FLOAT_LESS,
    ORDER_EQUAL,
    ORDER_TEXT_COMPARE,
    DUP_DUPLICATE,
    INTERVALLUM_INTERVALLUM,
    INTERVALLUM_VALUE,
    TUPLE_GENUS_KEY,
    TUPLE_KEY,
    TUPLE_PARTS,
    INSTANS_PARSE,
    VALOR_STRUCT,
    SERMO_SERMO,
    SERMO_MEUS,
    SERMO_TUUS,
    SERMO_SCRINIUM,
    SERMO_STATUS,
    SERMO_REQUEST,
    SERMO_ITEM,
    SERMO_BYTE,
    SERMO_BULK,
    SERMO_DONE,
    SERMO_ERROR,
    SERMO_CANCEL,
    SERMO_CONVERSATION,
    SERMO_CONV_ITEM,
    SERMO_OPEN,
    SERMO_SET_OPENER,
    SERMO_DISPATCH,
    SERMO_INSTALL_ROUTES,
    SERMO_AS_MEUS,
    SERMO_AS_TUUS,
    SERMO_READ_TEXT,
    SERMO_READ_TEXT_RESULT,
    SERMO_FAULT_RECORD,
    SERMO_READ_NUMERUS,
    SERMO_DRAIN,
    SERMO_READ_VALUE,
    SERMO_READ_VALUE_RESULT,
    SERMO_READ_BOOL,
    SERMO_READ_BOOL_RESULT,
    SERMO_READ_FLOAT,
    SERMO_READ_FLOAT_RESULT,
    SERMO_READ_BYTES,
    SERMO_READ_BYTES_RESULT,
    SERMO_READ_STRINGS,
    SERMO_READ_STRINGS_RESULT,
    SERMO_READ_AS,
    SERMO_READ_AS_RESULT,
    SERMO_DRAIN_RESULT,
    SERMO_READ_LINE,
    TRAP_TEXT_WINDOW,
    TEXT_SPLIT,
    TEXT_UPPER,
    TEXT_LOWER,
    TEXT_PARSE_FLOAT,
    TEXT_FLOAT,
    REGEX_MATCHES,
    REGEX_FIND,
    REGEX_FIND_ALL,
    REGEX_SPLIT,
    REGEX_REPLACE,
    REGEX_REPLACE_WITH,
    REGEX_ESCAPE,
    REGEX_MATCH_TEXT,
    REGEX_MATCH_START,
    REGEX_MATCH_END,
    REGEX_MATCH_GROUP,
    REGEX_MATCH_NAMED,
    REGEX_REGEX,
    REGEX_MATCH,
    REGEX_NEW,
    REGEX_REPLACE_EACH,
    REGEX_ERROR,
    REGEX_COMPILE,
    REGEX_COMPILE_OR_TRAP,
    REGEX_LITERAL_OR_TRAP,
];

/// The TypeScript runtime's word-ring operations (T1-T6). These are ring
/// functions, not width-policy stores: no `HirStoreSite` is required to spell
/// an operation in the ring. The runtime generates all twelve operations for
/// each of the eight signed/unsigned widths from one width/operation loop.
const fn modulus_ts_row(name: &'static str) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Modulus,
        name,
        kind: RuntimeKind::Fn,
        targets: &[Target::HirTypeScript],
    }
}

/// One runtime namespace: a module in Rust, a namespace export in TypeScript,
/// a name prefix in Go.
///
/// Ordered by spelling so a recorded set iterates alphabetically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    Exact,
    Dec,
    Inf,
    Format,
    Modulus,
    Display,
    Order,
    Tensor,
    Sparsa,
    Vector,
    Matrix,
    Intervallum,
    Instans,
    Regex,
    Json,
    Valor,
    Tuple,
    Atomic,
    Dup,
    Result,
    Variant,
    Sermo,
    Routes,
    Gen,
    Float,
    Trap,
    Text,
}

impl Namespace {
    /// Every namespace, in declaration order.
    pub const ALL: [Namespace; 27] = [
        Namespace::Exact,
        Namespace::Dec,
        Namespace::Inf,
        Namespace::Format,
        Namespace::Modulus,
        Namespace::Display,
        Namespace::Order,
        Namespace::Tensor,
        Namespace::Sparsa,
        Namespace::Vector,
        Namespace::Matrix,
        Namespace::Intervallum,
        Namespace::Instans,
        Namespace::Regex,
        Namespace::Json,
        Namespace::Valor,
        Namespace::Tuple,
        Namespace::Atomic,
        Namespace::Dup,
        Namespace::Result,
        Namespace::Variant,
        Namespace::Sermo,
        Namespace::Routes,
        Namespace::Gen,
        Namespace::Float,
        Namespace::Trap,
        Namespace::Text,
    ];

    /// The `lower_snake_case` spelling of the namespace.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Namespace::Exact => "exact",
            Namespace::Dec => "dec",
            Namespace::Inf => "inf",
            Namespace::Format => "format",
            Namespace::Modulus => "modulus",
            Namespace::Display => "display",
            Namespace::Order => "order",
            Namespace::Tensor => "tensor",
            Namespace::Sparsa => "sparsa",
            Namespace::Vector => "vector",
            Namespace::Matrix => "matrix",
            Namespace::Intervallum => "intervallum",
            Namespace::Instans => "instans",
            Namespace::Regex => "regex",
            Namespace::Json => "json",
            Namespace::Valor => "valor",
            Namespace::Tuple => "tuple",
            Namespace::Atomic => "atomic",
            Namespace::Dup => "dup",
            Namespace::Result => "result",
            Namespace::Variant => "variant",
            Namespace::Sermo => "sermo",
            Namespace::Routes => "routes",
            Namespace::Gen => "gen",
            Namespace::Float => "float",
            Namespace::Trap => "trap",
            Namespace::Text => "text",
        }
    }
}

impl PartialOrd for Namespace {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Namespace {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

/// What a table row names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeKind {
    /// An ordinary helper function.
    Fn,
    /// A store-family helper (`store`, `wrap` or `clamp`): spellable only at a
    /// store site, through [`spell_store`].
    StoreFn,
    /// A runtime type.
    Type,
}

/// One row of the canonical helper table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeItem {
    pub ns: Namespace,
    /// `lower_snake_case` for a function, `UpperCamelCase` for a type.
    pub name: &'static str,
    pub kind: RuntimeKind,
    /// The targets whose runtime package carries the helper.
    pub targets: &'static [Target],
}

/// Why a row has no spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellError {
    /// A `StoreFn` row went through [`spell`]; use [`spell_store`].
    StoreOnly,
    /// A non-`StoreFn` row went through [`spell_store`]; use [`spell`].
    NotStore,
    /// The target has no runtime-helper spelling rule.
    NoRuntime(Target),
}

/// The spelling of a non-store helper in `target`'s generated source.
///
/// # Errors
///
/// [`SpellError::StoreOnly`] for a `StoreFn` row, [`SpellError::NoRuntime`]
/// for a target outside Rust, TypeScript, Go, Swift and Python.
pub fn spell(target: Target, item: &RuntimeItem) -> Result<String, SpellError> {
    if item.kind == RuntimeKind::StoreFn {
        return Err(SpellError::StoreOnly);
    }
    spelling(target, item)
}

/// The spelling of a store-family helper. The [`HirStoreSite`] is the witness
/// that the call sits at a store site; it does not change the spelling.
///
/// # Errors
///
/// [`SpellError::NotStore`] for a row that is not a `StoreFn`,
/// [`SpellError::NoRuntime`] for a target without a spelling rule.
pub fn spell_store(
    target: Target,
    item: &RuntimeItem,
    _site: &HirStoreSite,
) -> Result<String, SpellError> {
    if item.kind != RuntimeKind::StoreFn {
        return Err(SpellError::NotStore);
    }
    spelling(target, item)
}

fn spelling(target: Target, item: &RuntimeItem) -> Result<String, SpellError> {
    let ns = item.ns.as_str();
    let is_type = item.kind == RuntimeKind::Type;
    match target {
        // The display types stay in their module (`faber::display::DisplayTokens`);
        // every other Rust type is re-exported at the crate root.
        Target::HirRust if is_type && item.ns == Namespace::Display => {
            Ok(format!("faber::display::{}", upper_camel(item.name)))
        }
        Target::HirRust if is_type => Ok(format!("faber::{}", upper_camel(item.name))),
        Target::HirRust => Ok(format!("faber::{}::{}", rust_module(ns), item.name)),
        Target::HirTypeScript | Target::HirPython if is_type => {
            Ok(format!("{ns}.{}", upper_camel(item.name)))
        }
        Target::HirTypeScript => Ok(format!("{ns}.{}", lower_camel(item.name))),
        Target::HirGo => Ok(format!("rt.{}{}", upper_camel(ns), upper_camel(item.name))),
        Target::HirSwift if is_type => {
            Ok(format!("{}.{}", upper_camel(ns), upper_camel(item.name)))
        }
        Target::HirSwift => Ok(format!("{}.{}", upper_camel(ns), lower_camel(item.name))),
        Target::HirPython => Ok(format!("{ns}.{}", item.name)),
        _ => Err(SpellError::NoRuntime(target)),
    }
}

/// A namespace as a Rust module path segment: `gen` is a reserved keyword of the
/// 2024 edition, so the crate's module is `r#gen` and the path spells it raw.
fn rust_module(ns: &str) -> String {
    match ns {
        "gen" => format!("r#{ns}"),
        _ => ns.to_owned(),
    }
}

/// The names a user binding must not take in `target`'s generated source,
/// sorted and without duplicates (a colliding binding escapes with a trailing
/// `_`).
///
/// Rust reserves the crate name `faber` and Go the package name `rt`. The
/// other targets reserve the identifier each namespace is spelled by (the
/// lower-case namespace in TypeScript and Python, the `UpperCamel` enum name in
/// Swift) for every namespace that has a row carried by `target`.
#[must_use]
pub fn reserved_names(target: Target, items: &[RuntimeItem]) -> Vec<String> {
    let namespaces = || {
        items
            .iter()
            .filter(|item| item.targets.contains(&target))
            .map(|item| item.ns.as_str())
    };
    let names: BTreeSet<String> = match target {
        Target::HirRust => BTreeSet::from(["faber".to_owned()]),
        Target::HirGo => BTreeSet::from(["rt".to_owned()]),
        Target::HirTypeScript | Target::HirPython => namespaces().map(str::to_owned).collect(),
        Target::HirSwift => namespaces().map(upper_camel).collect(),
        _ => BTreeSet::new(),
    };
    names.into_iter().collect()
}

/// The namespaces an emission referenced, recorded as the emitter spells
/// helpers so the file's imports are exactly what it used.
#[derive(Debug, Default)]
pub struct RuntimeRefs {
    used: RefCell<BTreeSet<Namespace>>,
}

impl RuntimeRefs {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one use of `ns`.
    pub fn note(&self, ns: Namespace) {
        self.used.borrow_mut().insert(ns);
    }

    /// Spell a non-store helper for `target` and record its namespace: the
    /// one way an emitter writes a helper into generated source, so the file's
    /// imports are exactly the namespaces it spelled.
    ///
    /// # Errors
    ///
    /// The errors of [`spell`]; nothing is recorded on an error.
    pub fn spell(&self, target: Target, item: &RuntimeItem) -> Result<String, SpellError> {
        let spelled = spell(target, item)?;
        self.note(item.ns);
        Ok(spelled)
    }

    /// The recorded namespaces, sorted by spelling, each once.
    #[must_use]
    pub fn used(&self) -> Vec<Namespace> {
        self.used.borrow().iter().copied().collect()
    }
}

/// The Go import the recorded references need: [`GO_RUNTIME_IMPORT`] when any
/// namespace was recorded, otherwise none.
#[must_use]
pub fn go_imports(refs: &RuntimeRefs) -> Option<&'static str> {
    if refs.used.borrow().is_empty() {
        None
    } else {
        Some(GO_RUNTIME_IMPORT)
    }
}

/// The TypeScript import the recorded references need: one named import of
/// every recorded namespace from [`TS_RUNTIME_IMPORT`], sorted by spelling, or
/// none when nothing was recorded.
#[must_use]
pub fn ts_imports(refs: &RuntimeRefs) -> Option<String> {
    let used = refs.used();
    if used.is_empty() {
        return None;
    }
    let names: Vec<&str> = used.iter().map(|ns| ns.as_str()).collect();
    Some(format!(
        "import {{ {} }} from \"{TS_RUNTIME_IMPORT}\";",
        names.join(", ")
    ))
}

/// `checked_mul` to `CheckedMul`; an already `UpperCamel` name is unchanged.
fn upper_camel(name: &str) -> String {
    name.split('_').map(capitalize).collect()
}

/// `checked_mul` to `checkedMul`.
fn lower_camel(name: &str) -> String {
    let mut segments = name.split('_');
    let mut out = String::new();
    if let Some(first) = segments.next() {
        let mut chars = first.chars();
        if let Some(head) = chars.next() {
            out.extend(head.to_lowercase());
            out.push_str(chars.as_str());
        }
    }
    out.extend(segments.map(capitalize));
    out
}

fn capitalize(segment: &str) -> String {
    let mut chars = segment.chars();
    match chars.next() {
        Some(head) => head.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
#[path = "runtime_api_test.rs"]
mod tests;
