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

use std::collections::{HashMap, HashSet};

/// `fac` / `cape` / `iace` success binding in `match` arms.
pub const FAC_OK: &str = "ok";

/// `fac` / `cape` / `iace` error binding in `match` arms.
pub const FAC_ERR: &str = "err";

/// `@ cursor` yield accumulator.
pub const YIELDED: &str = "y0";

/// Go lazy `fiunt` yield callback.
pub const YIELD_FN: &str = "y1";

/// `lege` stdin scratch buffer.
pub const INPUT: &str = "u0";

/// Optional-chain / map-get scratch receiver.
pub const OPTIONAL: &str = "o0";

/// Optional-chain receiver captured before the chain's operands run.
pub const OPTIONAL_RECEIVER: &str = "o0r";

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

/// Copy-with-changes base copy (`Genus { … } ex source`); override
/// temporaries append an ordinal (`b0_1`, `b0_2`, …).
pub const UPDATE_BASE: &str = "b0";

/// Rust item-level compiler names. Unlike the temporaries above these name a
/// generic parameter and a struct field of the emitted item, not a local, so
/// they do not follow the `letter decimal+` scheme; they are spelled here once
/// and the Rust emitter references them rather than typing them.
///
/// The generic element parameter of a tensor edge-policy provider impl.
pub const RUST_TENSOR_ELEMENT_PARAM: &str = "__FaberTensorElement";

/// The `PhantomData` marker field of a record whose type parameters are not
/// otherwise used by a field.
pub const RUST_STRUCT_TYPE_MARKER_FIELD: &str = "__faber_type_marker";

/// Every fixed hygiene name; a [`TempScope`] never hands one out as a temporary.
const FIXED_NAMES: [&str; 15] = [
    FAC_OK,
    FAC_ERR,
    YIELDED,
    YIELD_FN,
    INPUT,
    OPTIONAL,
    OPTIONAL_RECEIVER,
    CLOSURE_ITEM,
    CLOSURE_ACC,
    CLOSURE_SPLIT,
    MAP_KEY,
    MAP_VAL,
    RUNES,
    SLICE_OUT,
    UPDATE_BASE,
];

/// Per-function compiler temporaries.
///
/// Names are `<letter><n>` with `n` counted per letter from `0` inside one
/// function, so a function's temporaries do not depend on the module's HIR ids.
/// A name is never one the function's user bindings or the fixed hygiene names
/// already hold: the compiler temporary adapts, a user identifier keeps its
/// spelling. One `(kind, key)` always returns one name within a scope.
#[derive(Debug, Clone)]
pub struct TempScope {
    taken: HashSet<String>,
    assigned: HashMap<(char, u32), String>,
    next: HashMap<char, u32>,
}

impl TempScope {
    /// Start a scope for one function. `user_names` are the names the function
    /// declares; they and the fixed hygiene names are skipped by the allocator.
    #[must_use]
    pub fn new<I, S>(user_names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut taken: HashSet<String> =
            FIXED_NAMES.iter().map(|name| (*name).to_owned()).collect();
        taken.extend(user_names.into_iter().map(|name| name.as_ref().to_owned()));
        Self {
            taken,
            assigned: HashMap::new(),
            next: HashMap::new(),
        }
    }

    /// The temporary for `(kind, key)`, allocating the next free `<kind><n>` on
    /// first use. `kind` is one of the existing hygiene letters.
    pub fn name(&mut self, kind: char, key: u32) -> String {
        if let Some(existing) = self.assigned.get(&(kind, key)) {
            return existing.clone();
        }
        let counter = self.next.entry(kind).or_insert(0);
        let name = loop {
            let candidate = format!("{kind}{counter}");
            *counter += 1;
            if !self.taken.contains(&candidate) {
                break candidate;
            }
        };
        self.taken.insert(name.clone());
        self.assigned.insert((kind, key), name.clone());
        name
    }
}

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

/// `perge`-through-`fac` flag and wrapper-loop label (ruling R3). Labels
/// live in their own namespace on every target, so one name serves both.
#[must_use]
pub fn perge_route(route: u32) -> String {
    format!("k{route}")
}

/// Left/right operand bindings for an IEEE 754 float `minimum`/`maximum`.
#[must_use]
pub fn extremum_operands(id: u32) -> (String, String) {
    (format!("b{id}"), format!("r{id}"))
}

#[cfg(test)]
#[path = "hygiene_test.rs"]
mod tests;
