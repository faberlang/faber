//! Property-based tests for the types lattice.
//!
//! Verifies core lattice invariants across the type space:
//!
//! **Reflexivity** – every type `A` satisfies `is_assignable(A, A)`.
//!
//! **Transitivity** – over proper subtype edges, excluding coercions and the
//! exact Octeti/`lista<numerus<u8>>` compatibility bridge.

use crate::index::IndexExpr;
use crate::instans_precision::InstansPrecision;
use crate::numeric_width::NumericWidth;
use crate::{ParamType, Primitive, SemanticMutability, SemanticParamMode, Type, TypeId, TypeTable};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use std::fmt::Write as _;

/// Build a type table that covers the lattice space.
///
/// Returns the table and a vector of its `TypeId`s so the test can randomly
/// sample from the full space. The set includes:
///
/// - All 14 primitive types
/// - All `numerus<W>` / `fractus<W>` sized variants (i8 … i128, u8 … u128, f32, f64)
/// - All `instans<N>` precision variants
/// - Collection wrappers (`Array`, `Map`, `Set`, `Option`, `Promissum`, `Cursor`)
/// - Reference types (immutable and mutable)
/// - Indexed types (`Tensor`, `Vector`, `Matrix`, `Sparsa`)
/// - `Atomic<T>`, `Intervallum<T>`, `ModularWord`
/// - A function signature type
/// - A structural record
/// - A union type
fn build_comprehensive_table() -> (TypeTable, Vec<TypeId>) {
    let mut t = TypeTable::new();
    let mut ids: Vec<TypeId> = Vec::new();

    // ── Primitive types ──────────────────────────────────────────────────
    for prim in [
        Primitive::Ascii,
        Primitive::Textus,
        Primitive::Numerus,
        Primitive::Fractus,
        Primitive::Bivalens,
        Primitive::Nihil,
        Primitive::Vacuum,
        Primitive::Numquam,
        Primitive::Ignotum,
        Primitive::Octeti,
        Primitive::Regex,
        Primitive::Match,
        Primitive::Json,
        Primitive::Valor,
        Primitive::Instans,
    ] {
        ids.push(t.primitive(prim));
    }

    let textus = t.primitive(Primitive::Textus);
    let numerus = t.primitive(Primitive::Numerus);
    let fractus = t.primitive(Primitive::Fractus);
    let bivalens = t.primitive(Primitive::Bivalens);
    let nihil = t.primitive(Primitive::Nihil);
    let octeti = t.primitive(Primitive::Octeti);
    let _ignotum = t.primitive(Primitive::Ignotum);
    let _valor = t.primitive(Primitive::Valor);
    let _instans = t.primitive(Primitive::Instans);

    // ── Sized numerics ───────────────────────────────────────────────────
    for w in [
        NumericWidth::I8,
        NumericWidth::I16,
        NumericWidth::I32,
        NumericWidth::I64,
        NumericWidth::U8,
        NumericWidth::U16,
        NumericWidth::U32,
        NumericWidth::U64,
    ] {
        ids.push(t.sized_numeric(Primitive::Numerus, w));
    }
    for w in [NumericWidth::F16, NumericWidth::F32, NumericWidth::F64] {
        ids.push(t.sized_numeric(Primitive::Fractus, w));
    }

    // ── Sized instans ────────────────────────────────────────────────────
    for p in [
        InstansPrecision::Millis,
        InstansPrecision::Micros,
        InstansPrecision::Nanos,
    ] {
        ids.push(t.sized_instans(p));
    }

    // ── Core compound types (primitive-wrapped) ──────────────────────────
    ids.push(t.array(textus));
    ids.push(t.array(numerus));
    ids.push(t.array(octeti));
    ids.push(t.map(textus, numerus));
    ids.push(t.set(fractus));
    ids.push(t.option(textus));
    ids.push(t.promissum(numerus));
    ids.push(t.cursor(textus));
    ids.push(t.reference(SemanticMutability::Immutable, numerus));
    ids.push(t.reference(SemanticMutability::Mutable, numerus));

    // ── Octeti <-> Array<u8> bridge ──────────────────────────────────────
    let u8_id = t.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    ids.push(u8_id);
    let lista_u8 = t.array(u8_id);
    ids.push(lista_u8);

    // ── Indexed types ────────────────────────────────────────────────────
    let lit2 = t.intern_index(IndexExpr::Literal(2));
    let lit3 = t.intern_index(IndexExpr::Literal(3));
    let shape_2x3 = t.intern_index(IndexExpr::Tuple(vec![lit2, lit3]));
    ids.push(t.tensor_with_shape(fractus, lit2));
    ids.push(t.vector_with_width(fractus, lit3));
    ids.push(t.matrix_with_shape(fractus, shape_2x3));

    // ── Sparsa ───────────────────────────────────────────────────────────
    ids.push(t.sparsa_with_shape(fractus, lit2));

    // ── Atomic, Intervallum ──────────────────────────────────────────────
    let i32 = t.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    ids.push(t.atomic(i32));
    ids.push(t.intervallum(numerus));

    // ── ModularWord ──────────────────────────────────────────────────────
    ids.push(t.intern(Type::ModularWord(NumericWidth::U32)));

    // ── Union (Bivalens ∪ Nihil) ─────────────────────────────────────────
    ids.push(t.intern(Type::Union(vec![bivalens, nihil])));

    // ── Function signature ───────────────────────────────────────────────
    let fn_id = t.function(crate::FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: bivalens,
        err: None,
        is_async: false,
        is_generator: false,
    });
    ids.push(fn_id);

    // ── Structural record ────────────────────────────────────────────────
    ids.push(t.record([
        (radix_lexer::Symbol(1), textus),
        (radix_lexer::Symbol(2), numerus),
    ]));

    // ── Nested compound ──────────────────────────────────────────────────
    let array_textus = t.array(textus);
    ids.push(t.option(array_textus));
    let option_numerus = t.option(numerus);
    ids.push(t.array(option_numerus));

    (t, ids)
}

// ── Reflexivity ─────────────────────────────────────────────────────────────

#[test]
fn reflexivity_holds_for_all_types() {
    let (table, ids) = build_comprehensive_table();

    let failures: Vec<(usize, TypeId)> = ids
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, id)| !table.is_assignable(*id, *id))
        .collect();

    assert!(
        failures.is_empty(),
        "reflexivity violated for {} type(s):\n{failures:#?}",
        failures.len(),
    );
}

// ── Transitivity (pure lattice) ─────────────────────────────────────────────
//
// Subtype transitivity is checked over proper subtype edges only. Two edge
// classes of `is_assignable` are deliberate non-subtype edges and are
// classified explicitly (never skipped by a blanket filter):
//
// - `OctetiBridge`: `octeti` and exactly `lista<numerus<u8>>` are mutually
//   assignable through D10.4 identity (`equals`), but the identity does not
//   compose with element widening: `octeti` is NOT assignable to
//   `lista<numerus>` (pinned by `octeti_u8_lista_bridge_is_outside_subtype_transitivity`).
// - `OptionUnbox`: `Option(T)` -> `T` is a bare-assign unwrapping coercion.
//   Combined with `nihil` -> `Option(T)` it would falsely imply `nihil` -> `T`
//   (pinned by `option_unboxing_does_not_leak_nihil_into_concrete`).

/// Replay seed for the sampled property. Override with `LATTICE_SEED=<u64>`
/// to replay a reported failure.
const DEFAULT_LATTICE_SEED: u64 = 0x7426_fc87_a11c_e5ed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoercionEdge {
    OctetiBridge,
    OptionUnbox,
}

/// Classify an assignable edge as a non-subtype coercion, or `None` when it
/// is a proper subtype edge.
fn coercion_edge(table: &TypeTable, from: TypeId, to: TypeId) -> Option<CoercionEdge> {
    if is_octeti_u8_lista_bridge(table, from, to) {
        return Some(CoercionEdge::OctetiBridge);
    }
    let unboxes = matches!(table.get(from), Type::Option(_))
        && !matches!(
            table.get(to),
            Type::Option(_) | Type::Union(_) | Type::Primitive(Primitive::Ignotum)
        );
    unboxes.then_some(CoercionEdge::OptionUnbox)
}

/// Return every `(a, b, c)` with `a <: b`, `b <: c` over proper subtype edges
/// but not `a <: c`.
fn transitivity_violations(
    table: &TypeTable,
    triples: impl Iterator<Item = (TypeId, TypeId, TypeId)>,
) -> Vec<(TypeId, TypeId, TypeId)> {
    triples
        .filter(|&(a, b, c)| {
            table.is_assignable(a, b)
                && table.is_assignable(b, c)
                && coercion_edge(table, a, b).is_none()
                && coercion_edge(table, b, c).is_none()
                && !table.is_assignable(a, c)
        })
        .collect()
}

fn assert_no_violations(table: &TypeTable, violations: &[(TypeId, TypeId, TypeId)], context: &str) {
    if violations.is_empty() {
        return;
    }
    let mut msg = format!(
        "transitivity violated for {} triple(s) where A <: B and B <: C but not A <: C ({context}):\n",
        violations.len()
    );
    for (i, (a, b, c)) in violations.iter().enumerate().take(10) {
        let _ = writeln!(
            msg,
            "  [{i}]  A={a:?} ({})  <:  B={b:?} ({})  <:  C={c:?} ({})",
            table.get(*a).name(),
            table.get(*b).name(),
            table.get(*c).name(),
        );
    }
    if violations.len() > 10 {
        let _ = writeln!(msg, "  … and {} more", violations.len() - 10);
    }
    panic!("{msg}");
}

#[test]
fn transitivity_holds_for_random_triples() {
    let seed = std::env::var("LATTICE_SEED")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LATTICE_SEED);
    let (table, ids) = build_comprehensive_table();
    let mut rng = StdRng::seed_from_u64(seed);
    let triples: Vec<(TypeId, TypeId, TypeId)> = (0..10_000)
        .map(|_| {
            (
                *ids.choose(&mut rng).unwrap(),
                *ids.choose(&mut rng).unwrap(),
                *ids.choose(&mut rng).unwrap(),
            )
        })
        .collect();

    let violations = transitivity_violations(&table, triples.into_iter());
    assert_no_violations(&table, &violations, &format!("seed={seed}"));
}

#[test]
fn transitivity_holds_exhaustively_over_comprehensive_table() {
    let (table, ids) = build_comprehensive_table();
    let mut triples = Vec::with_capacity(ids.len().pow(3));
    for &a in &ids {
        for &b in &ids {
            for &c in &ids {
                triples.push((a, b, c));
            }
        }
    }

    let violations = transitivity_violations(&table, triples.into_iter());
    assert_no_violations(&table, &violations, "exhaustive");
}

fn is_octeti_u8_lista_bridge(table: &TypeTable, from: TypeId, to: TypeId) -> bool {
    let is_octeti = |id| matches!(table.get(id), Type::Primitive(Primitive::Octeti));
    let is_u8_lista = |id| {
        matches!(table.get(id), Type::Array(inner)
            if matches!(table.get(*inner), Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8)))
    };

    (is_octeti(from) && is_u8_lista(to)) || (is_u8_lista(from) && is_octeti(to))
}

#[test]
fn octeti_u8_lista_bridge_is_outside_subtype_transitivity() {
    let mut table = TypeTable::new();
    let octeti = table.primitive(Primitive::Octeti);
    let u8_type = table.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let lista_u8 = table.array(u8_type);
    let numerus = table.primitive(Primitive::Numerus);
    let lista_numerus = table.array(numerus);

    assert!(table.is_assignable(octeti, lista_u8));
    assert!(table.is_assignable(lista_u8, lista_numerus));
    assert!(!table.is_assignable(octeti, lista_numerus));
}

// ── Option coercion edges (non-lattice, must not falsely imply transitivity) ─

#[test]
fn option_unboxing_does_not_leak_nihil_into_concrete() {
    // Nihil → Option(T) is valid, and Option(T) → T via unboxing, but Nihil → T
    // must NOT hold (null must not leak into a non-optional slot).
    let mut table = TypeTable::new();
    let nihil = table.primitive(Primitive::Nihil);
    let textus = table.primitive(Primitive::Textus);
    let opt_textus = table.option(textus);

    assert!(table.is_assignable(nihil, opt_textus), "Nihil <: Option(T)");
    assert!(
        table.is_assignable(opt_textus, textus),
        "Option(T) <: T via unboxing",
    );
    assert!(
        !table.is_assignable(nihil, textus),
        "Nihil must NOT be assignable to non-optional T",
    );
}

#[test]
fn option_to_non_option_via_unboxing_is_not_reflexive() {
    // Option(T) unboxing is a coercion, not an identity: `equals(Option(T), T)`
    // is false.
    let mut table = TypeTable::new();
    let textus = table.primitive(Primitive::Textus);
    let opt = table.option(textus);

    assert!(!table.equals(opt, textus), "Option(T) != T structurally");
    assert!(
        table.is_assignable(opt, textus),
        "Option(T) <: T via coercion"
    );
}

// ── Transitivity (exhaustive, small lattice) ────────────────────────────────

#[test]
fn transitivity_holds_for_primitive_lattice() {
    let table = TypeTable::new();
    let prims: Vec<TypeId> = [
        Primitive::Ascii,
        Primitive::Textus,
        Primitive::Numerus,
        Primitive::Fractus,
        Primitive::Bivalens,
        Primitive::Nihil,
        Primitive::Vacuum,
        Primitive::Numquam,
        Primitive::Ignotum,
        Primitive::Octeti,
        Primitive::Regex,
        Primitive::Match,
        Primitive::Json,
        Primitive::Valor,
        Primitive::Instans,
    ]
    .into_iter()
    .map(|p| table.primitive(p))
    .collect();

    let mut violations = Vec::new();
    for a in &prims {
        for b in &prims {
            for c in &prims {
                if table.is_assignable(*a, *b)
                    && table.is_assignable(*b, *c)
                    && !table.is_assignable(*a, *c)
                {
                    violations.push((*a, *b, *c));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "primitive lattice transitivity violated for {} triple(s):\n{violations:#?}",
        violations.len(),
    );
}

// ── Numeric widening chain transitivity ─────────────────────────────────────

#[test]
fn numeric_widening_chain_is_transitive() {
    let mut table = TypeTable::new();
    let widths = [
        NumericWidth::I8,
        NumericWidth::I16,
        NumericWidth::I32,
        NumericWidth::I64,
    ];
    let numerics: Vec<TypeId> = widths
        .iter()
        .map(|w| table.sized_numeric(Primitive::Numerus, *w))
        .collect();

    // Every pair (narrower, wider) is assignable, and the chain is transitive.
    for a in &numerics {
        for b in &numerics {
            for c in &numerics {
                assert!(
                    !(table.is_assignable(*a, *b)
                        && table.is_assignable(*b, *c)
                        && !table.is_assignable(*a, *c)),
                    "numeric widening chain transitivity violated for {:?} <: {:?} <: {:?}",
                    widths[numerics.iter().position(|x| x == a).unwrap()],
                    widths[numerics.iter().position(|x| x == b).unwrap()],
                    widths[numerics.iter().position(|x| x == c).unwrap()],
                );
            }
        }
    }

    // Same for fractus (F32, F64)
    let f32 = table.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let f64 = table.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    assert!(table.is_assignable(f32, f64));
    assert!(!table.is_assignable(f64, f32));
    assert!(table.is_assignable(f32, f32));
    assert!(table.is_assignable(f64, f64));
}

// ── Instans precision chain transitivity ────────────────────────────────────

#[test]
fn instans_widening_chain_is_transitive() {
    let mut table = TypeTable::new();
    let bare = table.primitive(Primitive::Instans);
    let millis = table.sized_instans(InstansPrecision::Millis);
    let micros = table.sized_instans(InstansPrecision::Micros);
    let nanos = table.sized_instans(InstansPrecision::Nanos);

    let chain = [bare, millis, micros, nanos];

    for i in 0..chain.len() {
        for j in i..chain.len() {
            assert!(
                table.is_assignable(chain[i], chain[j]),
                "expected {:?} <: {:?} in instans chain",
                chain[i],
                chain[j],
            );
        }
    }

    // Transitivity: for any A <: B <: C in the chain, A <: C must hold.
    for a_idx in 0..chain.len() {
        for b_idx in a_idx..chain.len() {
            for c_idx in b_idx..chain.len() {
                let a = chain[a_idx];
                let b = chain[b_idx];
                let c = chain[c_idx];
                assert!(
                    table.is_assignable(a, b) && table.is_assignable(b, c),
                    "precondition: chain[{a_idx}] <: chain[{b_idx}] <: chain[{c_idx}]"
                );
                assert!(
                    table.is_assignable(a, c),
                    "instans chain transitivity: chain[{a_idx}] <: chain[{c_idx}] should hold (via chain[{b_idx}])",
                );
            }
        }
    }
}

// ── Compound reflexivity ────────────────────────────────────────────────────

#[test]
fn compound_reflexivity_holds_exhaustively() {
    // Build types and verify EVERY stored type satisfies is_assignable(self, self).
    // This catches issues with structural-recursive type containers whose
    // equals/is_assignable may fail on self-comparison for certain shapes.
    let mut table = TypeTable::new();
    let textus = table.primitive(Primitive::Textus);
    let numerus = table.primitive(Primitive::Numerus);

    // Manually intern a wide variety of compound types
    let mut type_ids = vec![
        table.array(textus),
        table.map(textus, numerus),
        table.set(numerus),
        table.option(textus),
        table.promissum(numerus),
        table.cursor(textus),
    ];

    let i32 = table.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    type_ids.push(table.atomic(i32));
    type_ids.push(table.intervallum(numerus));
    type_ids.push(table.intern(Type::ModularWord(NumericWidth::U32)));

    let f32 = table.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let lit1 = table.intern_index(IndexExpr::Literal(1));
    let lit2 = table.intern_index(IndexExpr::Literal(2));
    let tuple = table.intern_index(IndexExpr::Tuple(vec![lit1, lit2]));
    type_ids.push(table.tensor_with_shape(f32, lit1));
    type_ids.push(table.vector_with_width(f32, lit2));
    type_ids.push(table.matrix_with_shape(f32, tuple));
    type_ids.push(table.sparsa_with_shape(f32, lit1));

    type_ids.push(table.reference(SemanticMutability::Immutable, numerus));
    type_ids.push(table.reference(SemanticMutability::Mutable, numerus));

    type_ids.push(table.intern(Type::Union(vec![textus, numerus])));

    type_ids.push(table.function(crate::FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Ref,
            optional: false,
            sponte: false,
        }],
        ret: textus,
        err: None,
        is_async: false,
        is_generator: false,
    }));

    type_ids.push(table.record([
        (radix_lexer::Symbol(1), textus),
        (radix_lexer::Symbol(2), numerus),
    ]));

    for id in &type_ids {
        assert!(
            table.is_assignable(*id, *id),
            "reflexivity failed for {:?}",
            table.get(*id),
        );
    }
}

// ── Helper: short name for a Type (used in violation diagnostics) ───────────

trait TypeName {
    fn name(&self) -> &'static str;
}

impl TypeName for Type {
    fn name(&self) -> &'static str {
        match self {
            Type::Primitive(p) => match p {
                Primitive::Ascii => "ascii",
                Primitive::Textus => "textus",
                Primitive::Numerus => "numerus",
                Primitive::Fractus => "fractus",
                Primitive::Bivalens => "bivalens",
                Primitive::Nihil => "nihil",
                Primitive::Vacuum => "vacuum",
                Primitive::Numquam => "numquam",
                Primitive::Ignotum => "ignotum",
                Primitive::Octeti => "octeti",
                Primitive::Regex => "regex",
                Primitive::Match => "match",
                Primitive::Json => "json",
                Primitive::Valor => "valor",
                Primitive::Instans => "instans",
                Primitive::Littera => "littera",
            },
            Type::SizedNumeric(p, w) => {
                // Leak for diagnostic brevity
                Box::leak(format!("{p:?}<{w:?}>").into_boxed_str())
            }
            Type::Bits(w) => w.faber_marker(),
            Type::SizedInstans(p) => Box::leak(format!("instans<{p:?}>").into_boxed_str()),
            Type::Array(_) => "lista",
            Type::BoundedArray { .. } => "lista_n",
            Type::Queue(_) => "queue",
            Type::BoundedQueue { .. } => "queue_n",
            Type::Stack(_) => "stack",
            Type::BoundedStack { .. } => "stack_n",
            Type::BoundedTextus { .. } => "textus_n",
            Type::BoundedAscii { .. } => "ascii_n",
            Type::BoundedOcteti { .. } => "octeti_n",
            Type::Map(_, _) => "tabula",
            Type::Ledger(_, _) => "tabulae",
            Type::Option(_) => "option",
            Type::Set(_) => "copia",
            Type::Promissum(_) | Type::PromissumFailable(_, _) => "promissum",
            Type::Cursor(_) => "cursor",
            Type::AsyncCursor(_, _) => "async_cursor",
            Type::Tensor(_, _) => "tensor",
            Type::Vector(_, _) => "vector",
            Type::Matrix(_, _) => "matrix",
            Type::Sparsa(_, _) => "sparsa",
            Type::Atomic(_) => "atomic",
            Type::Intervallum(_) => "intervallum",
            Type::ModularWord(_) => "modular",
            Type::SaturatingWord(_) => "saturating",
            Type::PolicyFloat(_, _) => "policy_float",
            Type::TensorEdgePolicy(_) => "tensor_edge_policy",
            Type::Union(_) => "union",
            Type::Intersection(_) => "intersection",
            Type::Ref(_, _) => "ref",
            Type::Func(_) => "func",
            Type::Record(_) => "record",
            Type::Alias(_, _) => "alias",
            Type::Struct(_) => "struct",
            Type::Enum(_) => "enum",
            Type::Interface(_) => "interface",
            Type::Param(_) => "param",
            Type::Applied(_, _) => "applied",
            Type::Infer(_) => "infer",
            Type::InferUnion(_) => "infer_union",
            Type::MarkerHole { .. } => "marker_hole",
            Type::Tuple(_) => "tuple",
            Type::Error => "error",
        }
    }
}
