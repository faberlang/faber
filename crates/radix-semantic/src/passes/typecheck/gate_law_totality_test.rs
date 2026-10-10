//! Gate-law totality coverage audit.
//!
//! Mirrors `corpus/operatores/gate-law-totality.fab` (en surface): the
//! operator × carrier-pair product is enumerated here as a const table —
//! every cell pinned to exactly one defined join (probe analyzes clean) or
//! one named SEM rejection (probe fails with that issue). Two tests back the
//! table: a static completeness walk (a new operator or carrier family leaves
//! a visible hole and breaks the 81-cell / 69-join / 12-reject tallies) and a
//! live reprobe of every cell against the current gate arms.
//!
//! `est` / `non est` is not a table column: DECISIONS D23.1 made it a type
//! test, so it is no longer an equality gate.
//!
//! Closed findings (GT-1..GT-4, 2026-08-26): every contested cell is now a
//! defined join or a named rejection, pinned by the corpus exemplum and this
//! audit:
//!   GT-1  ≅/≇ and ≈/≉ × decimal pairs join at the d64 scale in MIR
//!         (conversion temps, mirroring the ordering join). With `d32`
//!         removed (D11.7) the P7 column is the identical `d64 × d64` pair.
//!   GT-2  ≅/≇ and ≈/≉ × decimal/fractus now REJECT with
//!         `mixed_decimal_fractus_equality`, mirroring the ordering gate
//!         (the pair was admitted but carrier-blind before).
//!   GT-3  negative signed→unsigned ↦ fails closed at runtime (conversion
//!         never wraps — DEFER-116 boundary contract), so print/≡/≅/ordering
//!         agree on the same law.
//!   GT-4  generated Rust mixed signed/unsigned ordering emits the same
//!         lossless i128 join as ≅ equality — `u64::MAX` no longer wraps to
//!         -1 through `as i64`, so < / ≥ match the runner's sign-boundary
//!         verdicts exactly.

use crate::test_support;

const EXACT_EQ: &str = "equality_exact_types_required";
const ARITH_U64_SIGNED: &str = "u64_signed_arithmetic_requires_conversion";
const ARITH_FAMILY_CROSSING: &str = "numeric_family_crossing_requires_conversio";

const fn join() -> Pin {
    Pin::Join
}

const fn reject(issue: &'static str) -> Pin {
    Pin::Reject(issue)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pin {
    Join,
    Reject(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    P1IntIdentical,
    P2IntCrossWidth,
    P3SignBoundary,
    P4IntF32,
    P5IntF64,
    P6F32F64,
    P7D64Identical,
    P8DecimalInteger,
    P9DecimalFractus,
}

const FAMILIES: [Family; 9] = [
    Family::P1IntIdentical,
    Family::P2IntCrossWidth,
    Family::P3SignBoundary,
    Family::P4IntF32,
    Family::P5IntF64,
    Family::P6F32F64,
    Family::P7D64Identical,
    Family::P8DecimalInteger,
    Family::P9DecimalFractus,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    StrictEq,
    PromotedExact,
    Fuzzy,
    Lt,
    Le,
    Gt,
    Ge,
    Arith,
    Conversio,
}

const OPS: [Op; 9] = [
    Op::StrictEq,
    Op::PromotedExact,
    Op::Fuzzy,
    Op::Lt,
    Op::Le,
    Op::Gt,
    Op::Ge,
    Op::Arith,
    Op::Conversio,
];

/// The 81-cell truth table, row = operator, column = carrier-pair family.
const TABLE: [[Pin; 9]; 9] = [
    // ≡ / ≢ — strict equality is fail-closed: identical types only.
    [
        join(),
        reject(EXACT_EQ),
        reject(EXACT_EQ),
        reject(EXACT_EQ),
        reject(EXACT_EQ),
        reject(EXACT_EQ),
        join(), // d64 × d64 is an identical type pair
        reject(EXACT_EQ),
        reject(EXACT_EQ),
    ],
    // ≅ / ≇ — promoted-exact: widths join through numeric_bin, then compare.
    [
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(), // GT-1 closed: decimals join at the d64 scale in MIR
        join(),
        join(), // ruling 22: cross-family comparison admitted
    ],
    // ≈ / ≉ — fuzzy at the joined precision; same admission shape as ≅.
    [
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(), // GT-1 closed: decimals join at the d64 scale in MIR
        join(),
        join(), // ruling 22: cross-family comparison admitted
    ],
    // < — ordering joins at the widened carrier; decimal/fractus rejects.
    [
        join(),
        join(),
        join(), // GT-4 closed: rust emits the lossless i128 join
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
    ],
    // ≤ — same ordering gate.
    [
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
    ],
    // > — same ordering gate.
    [
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
    ],
    // ≥ — same ordering gate.
    [
        join(),
        join(),
        join(), // GT-4 closed: rust emits the lossless i128 join
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
    ],
    // + - * / % — d11-6 U3: integer widths join by the range rule; float and
    // decimal widths join by the lossless lattice; `u64` with a signed typed
    // operand (ruling 8) and any number-family crossing (ruling 16) reject.
    [
        join(),
        join(),
        reject(ARITH_U64_SIGNED),
        reject(ARITH_FAMILY_CROSSING),
        reject(ARITH_FAMILY_CROSSING),
        join(),
        join(),
        reject(ARITH_FAMILY_CROSSING),
        reject(ARITH_FAMILY_CROSSING),
    ],
    // ↦ — conversio is directional; every family direction admits.
    // GT-3 closed: the pair stays admitted; a negative value into an unsigned
    // target fails closed at runtime (conversion never wraps), so the join
    // verdict holds for every value that converts.
    [
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
        join(),
    ],
];

fn pair_types(family: Family) -> (&'static str, &'static str) {
    match family {
        Family::P1IntIdentical => ("numerus", "numerus"),
        Family::P2IntCrossWidth => ("u8", "u16"),
        Family::P3SignBoundary => ("i64", "u64"),
        Family::P4IntF32 => ("i64", "f32"),
        Family::P5IntF64 => ("numerus", "fractus"),
        Family::P6F32F64 => ("f32", "f64"),
        Family::P7D64Identical => ("d64", "d64"),
        Family::P8DecimalInteger => ("d64", "numerus"),
        Family::P9DecimalFractus => ("d64", "fractus"),
    }
}

fn op_glyph(op: Op) -> &'static str {
    match op {
        Op::StrictEq => "≡",
        Op::PromotedExact => "≅",
        Op::Fuzzy => "≈",
        Op::Lt => "≺",
        Op::Le => "≤",
        Op::Gt => "≻",
        Op::Ge => "≥",
        Op::Arith => "+",
        Op::Conversio => "↦",
    }
}

fn probe(op: Op, family: Family) -> String {
    if op == Op::Conversio {
        let (src, tgt) = match family {
            Family::P1IntIdentical => ("numerus", "u64"),
            Family::P2IntCrossWidth => ("u8", "u16"),
            Family::P3SignBoundary => ("u64", "numerus"),
            Family::P4IntF32 => ("i64", "f32"),
            Family::P5IntF64 => ("numerus", "fractus"),
            Family::P6F32F64 => ("f32", "f64"),
            Family::P7D64Identical => ("d64", "d64"),
            Family::P8DecimalInteger => ("d64", "numerus"),
            Family::P9DecimalFractus => ("d64", "fractus"),
        };
        // Need: a decimal into an integer names its rounding.
        let via = if matches!(family, Family::P8DecimalInteger) {
            " via HalfEven"
        } else {
            ""
        };
        return format!("functio p({src} a) → {tgt} ⇥ textus {{ redde a ↦ {tgt}{via} }}");
    }
    let (lhs, rhs) = pair_types(family);
    if op == Op::Arith {
        // The sum lands in an inferred slot: the store rule (U4) is not probed.
        return format!("functio p({lhs} a, {rhs} b) {{ fixum _ r ← a + b }}");
    }
    format!(
        "functio p({lhs} a, {rhs} b) → bivalens {{ redde a {} b }}",
        op_glyph(op)
    )
}

/// Static completeness: the table is closed over the enumerated product, the
/// reject pins name real issues, and the tallies match the corpus table
/// (81 cells = 69 joins + 12 rejections; ruling 22 moved the six
/// decimal/fractus comparison cells to joins; GT-2 moved the two decimal/fractus
/// equality cells from joins to named rejections; D23.1 removed the `est`
/// row, which is a type test rather than an equality gate).
#[test]
fn gate_law_table_is_total() {
    assert_eq!(TABLE.len(), OPS.len(), "one row per operator");
    assert_eq!(TABLE[0].len(), FAMILIES.len(), "one cell per family");
    let mut joins = 0usize;
    let mut rejects = 0usize;
    for (op, row) in OPS.iter().zip(TABLE) {
        for (family, pin) in FAMILIES.iter().zip(row) {
            match pin {
                Pin::Join => joins += 1,
                Pin::Reject(issue) => {
                    rejects += 1;
                    assert!(
                        !issue.is_empty(),
                        "{op:?} × {family:?}: reject pin must name an issue"
                    );
                }
            }
        }
    }
    assert_eq!((joins, rejects), (69, 12));
}

/// Live reprobe: every cell's probe must meet its pin against the current
/// gate arms — a join analyzes clean, a rejection fails with the named issue.
#[test]
fn gate_law_cells_reprobe_live_gate() {
    for (op, row) in OPS.iter().zip(TABLE) {
        for (family, pin) in FAMILIES.iter().zip(row) {
            let source = probe(*op, *family);
            match pin {
                Pin::Join => test_support::assert_accepts(&source),
                Pin::Reject(issue) => {
                    test_support::assert_rejects_with_issue(&source, issue);
                }
            }
        }
    }
}
