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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hygiene_names_use_single_letter_prefixes() {
        assert_eq!(temp(3), "t3");
        assert_eq!(item(7), "i7");
        assert_eq!(range_start(0), "s0");
        assert_eq!(range_end(0), "e0");
        assert_eq!(range_step(0), "d0");
        assert_eq!(filter(2), "f2");
        assert_eq!(text_arg(5), "a5");
        assert_eq!(FAC_OK, "ok");
        assert_eq!(FAC_ERR, "err");
    }

    #[test]
    fn hygiene_temp_and_item_have_expected_format() {
        assert_eq!(temp(0), "t0");
        assert_eq!(temp(99), "t99");
        assert_eq!(item(1), "i1");
        assert_eq!(item(100), "i100");
    }

    #[test]
    fn hygiene_range_names_have_expected_format() {
        assert_eq!(range_start(0), "s0");
        assert_eq!(range_end(0), "e0");
        assert_eq!(range_step(0), "d0");
        assert_eq!(range_limit(0), "l0");
        assert_eq!(range_cursor(0), "n0");
        assert_eq!(range_values(0), "v0");
    }

    #[test]
    fn hygiene_closure_and_text_names_have_expected_format() {
        assert_eq!(filter(2), "f2");
        assert_eq!(mapper(3), "m3");
        assert_eq!(reducer(4), "h4");
        assert_eq!(count(5), "c5");
        assert_eq!(text_arg(5), "a5");
        assert_eq!(text_sep(6), "j6");
        assert_eq!(text_old(7), "o7");
        assert_eq!(text_new(8), "p8");
        assert_eq!(rune_index(9), "n9");
    }

    #[test]
    fn hygiene_constants_are_correct() {
        assert_eq!(FAC_OK, "ok");
        assert_eq!(FAC_ERR, "err");
        assert_eq!(YIELDED, "y0");
        assert_eq!(INPUT, "u0");
        assert_eq!(OPTIONAL, "o0");
        assert_eq!(CLOSURE_ITEM, "x");
        assert_eq!(CLOSURE_ACC, "a");
        assert_eq!(CLOSURE_SPLIT, "q");
        assert_eq!(MAP_KEY, "k");
        assert_eq!(MAP_VAL, "w");
        assert_eq!(RUNES, "g0");
        assert_eq!(SLICE_OUT, "out0");
    }
}
