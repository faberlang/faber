//! Shared HIR emission hygiene names for Rust, Go, and TypeScript backends.
//!
//! TARGET: rust, go, ts
//! User declarations keep [`crate::names::NameCatalog`] spellings. This module
//! names compiler-inserted temporaries with one ASCII letter plus a disambiguator.
//!
//! GRAMMAR:
//!   hygieneName :← letter decimal+
//!
//! Runtime prelude symbols (frame shim helpers) intentionally stay
//! in the runtime namespace and are not routed through this ledger.

/// `fac` / `cape` / `iace` success binding in `match` arms.
pub const FAC_OK: &str = "ok";

/// `fac` / `cape` / `iace` error binding in `match` arms.
pub const FAC_ERR: &str = "err";

/// `@ cursor` yield accumulator.
pub const YIELDED: &str = "y0";

/// `lege` stdin scratch buffer.
pub const INPUT: &str = "u0";

/// Optional-chain / map-get scratch receiver.
pub const OPTIONAL: &str = "o0";

/// `filter` / `map` / `fold` closure item parameter.
pub const CLOSURE_ITEM: &str = "x";

/// `fold` accumulator parameter.
pub const CLOSURE_ACC: &str = "a";

/// `split` element parameter.
pub const CLOSURE_SPLIT: &str = "q";

/// `verte` map literal key parameter.
pub const MAP_KEY: &str = "k";

/// `verte` map literal value parameter.
pub const MAP_VAL: &str = "w";

/// Go `[]rune` buffer in text slice lowering.
pub const RUNES: &str = "g0";

/// Go assembled rune output buffer.
pub const SLICE_OUT: &str = "out0";

/// Expression-or-receiver temporary (`struct`, `vec`, `list`, text base).
#[must_use]
pub fn temp(id: u32) -> String {
    format!("t{id}")
}

/// Transfer sink temporary (`⇇` single-evaluation binding).
#[must_use]
pub fn transfer_sink(id: u32) -> String {
    format!("z{id}")
}

/// Loop item scratch when the binding name is not emitted directly.
#[must_use]
pub fn item(def_id: u32) -> String {
    format!("i{def_id}")
}

/// Range / slice start bound.
#[must_use]
pub fn range_start(scope: u32) -> String {
    format!("s{scope}")
}

/// Range / slice end bound.
#[must_use]
pub fn range_end(scope: u32) -> String {
    format!("e{scope}")
}

/// Range step (delta).
#[must_use]
pub fn range_step(scope: u32) -> String {
    format!("d{scope}")
}

/// Range terminal limit.
#[must_use]
pub fn range_limit(scope: u32) -> String {
    format!("l{scope}")
}

/// Range loop cursor.
#[must_use]
pub fn range_cursor(scope: u32) -> String {
    format!("n{scope}")
}

/// Materialized range values (`itera ab` lowering).
#[must_use]
pub fn range_values(scope: u32) -> String {
    format!("v{scope}")
}

/// Predicate closure for `filter`.
#[must_use]
pub fn filter(id: u32) -> String {
    format!("f{id}")
}

/// Mapper closure for `map`.
#[must_use]
pub fn mapper(id: u32) -> String {
    format!("m{id}")
}

/// Count operand for `take` / `skip` style list ops.
#[must_use]
pub fn count(id: u32) -> String {
    format!("c{id}")
}

/// Reducer closure for `fold`.
#[must_use]
pub fn reducer(id: u32) -> String {
    format!("h{id}")
}

/// Text method scratch argument.
#[must_use]
pub fn text_arg(id: u32) -> String {
    format!("a{id}")
}

/// Text `split` separator scratch.
#[must_use]
pub fn text_sep(id: u32) -> String {
    format!("j{id}")
}

/// Text `replace` old substring scratch.
#[must_use]
pub fn text_old(id: u32) -> String {
    format!("o{id}")
}

/// Text `replace` new substring scratch.
#[must_use]
pub fn text_new(id: u32) -> String {
    format!("p{id}")
}

/// Go single-rune index scratch.
#[must_use]
pub fn rune_index(scope: u32) -> String {
    format!("n{scope}")
}

/// Left/right operand bindings for an IEEE 754 float `minimum`/`maximum`.
#[must_use]
pub fn extremum_operands(id: u32) -> (String, String) {
    (format!("b{id}"), format!("r{id}"))
}

#[cfg(test)]
#[path = "hygiene_test.rs"]
mod tests;
