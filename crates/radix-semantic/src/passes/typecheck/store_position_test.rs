//! d11-6 U4: every store is checked, at every position of the ruling-21
//! store table (`docs/factory/faber-language-decisions/d11-6-widening-delivery.md`
//! §3(a)).
//!
//! A store that may not fit is wrapped in an implicit store node
//! (`Conversio { implicit: Some(site), .. }`) whose target is the slot; a
//! store the range rule proves fits carries no node. Tests read the HIR after
//! typecheck and pin the node's presence (with its store kind) or its absence.

use super::test_support;
use crate::types::Type;
use crate::{NumericWidth, Primitive};
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{
    HirBinOp, HirConversioTarget, HirExpression, HirExpressionKind, HirStoreKind, HirStoreSite,
};
use radix_types::TypeId;

/// One implicit store node found in the analyzed HIR.
struct Store {
    site: HirStoreSite,
    target: TypeId,
}

#[derive(Default)]
struct Collector {
    stores: Vec<Store>,
    binary_ops: Vec<(HirBinOp, Option<TypeId>)>,
}

impl HirVisitor for Collector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Binary(op, ..) = &expr.kind {
            self.binary_ops.push((*op, expr.ty));
        }
        if let HirExpressionKind::Conversio {
            target: HirConversioTarget::Type(target),
            implicit: Some(site),
            ..
        } = &expr.kind
        {
            self.stores.push(Store {
                site: site.clone(),
                target: *target,
            });
        }
        walk_expr(self, expr);
    }
}

/// Analyze `source` (which must be accepted) and return its implicit stores
/// in source order, with the analyzed table for type inspection.
fn stores_of(source: &str) -> (Vec<Store>, test_support::AnalyzedFixture) {
    let result = test_support::analyze_ok_la(source);
    let hir = result.hir.as_ref().expect("hir");
    let mut collector = Collector::default();
    walk_program(&mut collector, hir);
    (collector.stores, result)
}

fn kinds(source: &str) -> Vec<HirStoreKind> {
    stores_of(source)
        .0
        .iter()
        .map(|store| store.site.kind)
        .collect()
}

/// Wrap statements in an entry block that has a wide `n` and two `u8`s.
fn entry(body: &str) -> String {
    format!("incipit {{\nsit numerus n ← 200\nvaria u8 x ← 100\nsit u8 y ← 50\n{body}\n}}\n")
}

fn assert_rejected(source: &str) {
    let result = test_support::analyze_source_la(source);
    assert!(!result.is_ok(), "expected a rejection");
}

fn assert_issue(source: &str, issue: &str) {
    test_support::assert_rejects_with_issue_la(source, issue);
}

#[test]
fn word_ring_skips_the_read_left_operand_of_division_and_right_shift() {
    let division = "incipit {\nsit modulus<u8> a ← 200\nsit modulus<u8> b ← 100\nsit modulus<u8> avg ← (a + b) / 2\nnota avg\n}\n";
    let result = test_support::analyze_ok_la(division);
    let hir = result.hir.as_ref().expect("hir");
    let mut collector = Collector::default();
    walk_program(&mut collector, hir);
    let division_ty = collector
        .binary_ops
        .iter()
        .find_map(|(op, ty)| (*op == HirBinOp::Div).then_some(*ty).flatten())
        .expect("division has a type");
    // Exact range typing widens the sum of two u8 operands before division.
    assert!(matches!(
        result.types.get(division_ty),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U16)
    ));
    assert!(!matches!(
        result.types.get(division_ty),
        Type::ModularWord(_)
    ));

    let shift =
        "incipit {\nsit modulus<u8> w2 ← 200\nsit modulus<u8> r ← (w2 ⇐ 1) ⇒ 1\nnota r\n}\n";
    let result = test_support::analyze_ok_la(shift);
    let hir = result.hir.as_ref().expect("hir");
    let mut collector = Collector::default();
    walk_program(&mut collector, hir);
    let shift_ty = collector
        .binary_ops
        .iter()
        .find_map(|(op, ty)| (*op == HirBinOp::Shr).then_some(*ty).flatten())
        .expect("right shift has a type");
    assert!(matches!(
        result.types.get(shift_ty),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U16)
    ));
    assert!(!matches!(result.types.get(shift_ty), Type::ModularWord(_)));
    let left_shift_ty = collector
        .binary_ops
        .iter()
        .find_map(|(op, ty)| (*op == HirBinOp::Shl).then_some(*ty).flatten())
        .expect("left shift has a type");
    assert!(matches!(
        result.types.get(left_shift_ty),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U16)
    ));

    let multiplication = "incipit {\nsit modulus<u8> a ← 200\nsit modulus<u8> b ← 100\nsit modulus<u8> rot ← (a + b) * 3\nnota rot\n}\n";
    let result = test_support::analyze_ok_la(multiplication);
    let hir = result.hir.as_ref().expect("hir");
    let mut collector = Collector::default();
    walk_program(&mut collector, hir);
    let multiplication_ty = collector
        .binary_ops
        .iter()
        .find_map(|(op, ty)| (*op == HirBinOp::Mul).then_some(*ty).flatten())
        .expect("multiplication has a type");
    assert!(matches!(
        result.types.get(multiplication_ty),
        Type::ModularWord(NumericWidth::U8)
    ));
}

// ---------------------------------------------------------------------------
// Rows 1-3: declaration, assignment, and the absence of a node
// ---------------------------------------------------------------------------

#[test]
fn row_01_declaration_narrowing_is_wrapped() {
    // `x * 3 / 2` is `u16` by the range rule; the store into `u8` is checked.
    let source = entry("sit u8 z ← x * 3 / 2");
    assert_eq!(kinds(&source), vec![HirStoreKind::Declaration]);
}

#[test]
fn row_01_widening_and_equal_stores_carry_no_node() {
    // `u8 - u8` is `i16`; into `i32` it cannot fail (ruling 12), and a `u8`
    // into a `u8` slot is the identity.
    let source = entry("sit i32 d ← x - y\nsit u8 e ← x");
    assert!(kinds(&source).is_empty());
}

#[test]
fn row_01_node_carries_the_slot_type_and_name() {
    let source = entry("sit u8 z ← n");
    let (stores, result) = stores_of(&source);
    assert_eq!(stores.len(), 1);
    let target = result.types.get(stores[0].target);
    assert!(matches!(
        target,
        Type::SizedNumeric(_, radix_types::NumericWidth::U8)
    ));
    assert!(
        stores[0].site.slot.is_some(),
        "the declared name rides the node"
    );
    assert!(stores[0].site.inferred_from.is_none());
}

/// Await-binding stores marked on their locals, in source order.
#[derive(Default)]
struct AwaitStores(Vec<Option<HirStoreSite>>);

impl HirVisitor for AwaitStores {
    fn visit_local(&mut self, local: &radix_hir::HirLocal) {
        if local.is_await {
            self.0.push(local.await_store.clone());
        }
        radix_hir::visit::walk_local(self, local);
    }
}

fn await_stores(source: &str) -> Vec<Option<HirStoreSite>> {
    let result = test_support::analyze_ok_la(source);
    let mut collector = AwaitStores::default();
    walk_program(&mut collector, result.hir.as_ref().expect("hir"));
    collector.0
}

#[test]
fn row_02_narrowing_await_bind_is_marked() {
    let source = "functio fetch() fiet → numerus {\n    redde 300\n}\n\
                  functio caller() fiet → vacuum {\n    sit u8 v ↢ fetch()\n    nota v\n}\n";
    let stores = await_stores(source);
    assert_eq!(stores.len(), 1);
    let site = stores[0].clone().expect("narrowing await-bind is marked");
    assert_eq!(site.kind, HirStoreKind::AwaitBinding);
    assert!(site.slot.is_some());
}

#[test]
fn row_02_assignable_await_bind_is_not_marked() {
    let source = "functio fetch() fiet → u8 {\n    redde 5\n}\n\
                  functio caller() fiet → vacuum {\n    sit i32 v ↢ fetch()\n    sit u8 w ↢ fetch()\n    nota v\n    nota w\n}\n";
    assert_eq!(await_stores(source), vec![None, None]);
}

#[test]
fn row_02_figendum_form_is_marked() {
    let source = "functio fetch() fiet → numerus {\n    redde 300\n}\n\
                  functio caller() fiet → vacuum {\n    figendum u8 v ← fetch()\n    nota v\n}\n";
    let stores = await_stores(source);
    assert_eq!(stores.len(), 1);
    assert_eq!(
        stores[0].clone().expect("marked").kind,
        HirStoreKind::AwaitBinding
    );
}

#[test]
fn row_03_assignment_narrowing_is_wrapped() {
    let source = entry("x ← n");
    assert_eq!(kinds(&source), vec![HirStoreKind::Assignment]);
}

#[test]
fn row_03_assignment_into_an_inferred_slot_names_the_source_expression() {
    // ruling 12(c): `_` takes the expression's type (`u16` here), and a later
    // store that does not fit is checked. The node names where the type came
    // from (ruling 19(i)).
    let source = entry("varia _ total ← x + y\ntotal ← n");
    let (stores, _) = stores_of(&source);
    assert_eq!(stores.len(), 1);
    assert_eq!(stores[0].site.kind, HirStoreKind::Assignment);
    assert!(stores[0].site.inferred_from.is_some());
}

#[test]
fn row_04_increment_is_its_own_checked_store() {
    // `x ↑` computes wide and stores back; there is no expression to wrap and
    // the checked add at `u8` is the store.
    let source = entry("x ↑\nx ↓");
    assert!(kinds(&source).is_empty());
}

// ---------------------------------------------------------------------------
// Rows 5-11: arguments, defaults, returns, closures, yields, throws
// ---------------------------------------------------------------------------

#[test]
fn row_05_call_arguments_are_stores() {
    let source = "functio take(u8 v) → vacuum {\n    nota v\n}\n\
                  incipit {\nsit numerus n ← 200\ntake(n)\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Argument]);
}

#[test]
fn row_05_method_arguments_are_stores() {
    let source = entry("varia lista<u8> xs ← [1]\nxs.appende(n)");
    assert_eq!(kinds(&source), vec![HirStoreKind::Argument]);
}

#[test]
fn row_05_a_widening_argument_carries_no_node() {
    let source = "functio take(i32 v) → vacuum {\n    nota v\n}\n\
                  incipit {\nsit u8 a ← 1\nsit u8 b ← 2\ntake(a - b)\n}\n";
    assert!(kinds(source).is_empty());
}

#[test]
fn row_06_parameter_default_is_a_store() {
    let fits = "functio f(u8 v vel 7) → u8 {\n    redde v\n}\n";
    assert!(kinds(fits).is_empty());
    let too_big = "functio f(u8 v vel 300) → u8 {\n    redde v\n}\n";
    assert_issue(too_big, "store_constant_out_of_range");
}

#[test]
fn row_07_return_is_a_store() {
    let source = "functio lata(numerus n) → u8 {\n    redde n\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Return]);
}

#[test]
fn row_08_reddet_is_a_return_store_on_resolution() {
    let source = "functio fetch() fiet → numerus {\n    redde 1\n}\n\
                  functio caller() fiet → u8 {\n    reddet fetch()\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Return]);
}

#[test]
fn row_09_closure_with_declared_return_is_a_store() {
    let source = entry("sit (numerus) → u8 f ← clausura numerus v → u8 { redde v }\nnota f(n)");
    assert_eq!(kinds(&source), vec![HirStoreKind::Return]);
}

#[test]
fn row_10_yield_is_a_store_against_the_element_type() {
    let source = "functio gignit(numerus n) fiunt → u8 {\n    cede n\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Yield]);
}

#[test]
fn row_10_yield_of_a_mismatched_element_is_rejected() {
    // The element type was unchecked before U4; a `textus` yield into a
    // numeric generator is now the ordinary mismatch.
    let source = "functio gignit() fiunt → u8 {\n    cede \"x\"\n}\n";
    assert_rejected(source);
}

#[test]
fn row_11_throw_is_a_store_against_the_declared_error_type() {
    let source = "functio iacit(numerus n) → numerus ⇥ u8 {\n    iace n\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Throw]);
}

// ---------------------------------------------------------------------------
// Rows 12-16: constructors, elements, positions, conversion default
// ---------------------------------------------------------------------------

const ARCA: &str = "genus Arca {\n    sit u8 a\n}\n";

#[test]
fn row_12_typed_constructor_fields_are_stores() {
    let source =
        format!("{ARCA}incipit {{\nsit numerus n ← 200\nsit Arca b ← Arca {{ a = n }}\n}}\n");
    assert_eq!(kinds(&source), vec![HirStoreKind::Field]);
}

#[test]
fn row_13_variant_fields_are_stores() {
    let source = "discretio Forma {\n    Plena { u8 res }\n}\n\
                  incipit {\nsit numerus n ← 200\nsit Forma f ← finge Plena { res = n }\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Field]);
}

#[test]
fn row_14_elements_against_a_declared_element_type_are_stores() {
    let source = entry("sit lista<u8> xs ← [n, 1]");
    assert_eq!(kinds(&source), vec![HirStoreKind::Element]);
}

#[test]
fn row_14_elements_with_no_declared_type_use_the_range_rule() {
    // No slot: `[x, m]` with `u8`, `i16` is typed by the smallest type holding
    // both elements, and nothing is narrowed.
    let source = entry("sit i16 m ← 5\nsit _ xs ← [x, m]");
    assert!(kinds(&source).is_empty());
}

#[test]
fn row_15_iuncta_positions_are_stores() {
    let source = entry("sit iuncta<u8, u8> t ← iuncta<u8, u8>[n, y]");
    assert_eq!(kinds(&source), vec![HirStoreKind::TuplePosition]);
}

#[test]
fn row_16_conversion_default_is_a_store_into_the_target() {
    let source = entry("sit u8 r ← n ↦ u8 ⊥ n");
    assert_eq!(kinds(&source), vec![HirStoreKind::ConversionDefault]);
}

// ---------------------------------------------------------------------------
// Rows 17-19a: compile-time stores (`=`)
// ---------------------------------------------------------------------------

#[test]
fn row_17_generis_constant_must_fit() {
    assert!(kinds("fixum u8 W = 200\nincipit {\nnota W\n}\n").is_empty());
    assert_issue(
        "fixum u8 W = 300\nincipit {\nnota W\n}\n",
        "store_constant_out_of_range",
    );
}

#[test]
fn row_17_generis_constant_must_fit_whatever_the_policy() {
    // A saturating or wrapping slot does not reduce an `=` constant.
    assert_rejected("fixum saturatus<u8> W = 300\nincipit {\nnota W\n}\n");
    assert_rejected("fixum modulus<u8> W = 300\nincipit {\nnota W\n}\n");
}

#[test]
fn row_18_field_default_constant_must_fit() {
    let fits = "genus G {\n    sit u8 b = 5\n}\n";
    assert!(kinds(fits).is_empty());
    let too_big = "genus G {\n    sit u8 b = 300\n}\n";
    assert_issue(too_big, "store_constant_out_of_range");
}

// Row 19 (enum member against a representation type) and row 19a
// (`fixum T x = e`) have no typecheck site yet: `ordo` has no representation
// type in the grammar and `fixum_decl` does not accept `=`. The rules for both
// are the `=` rule pinned by rows 17 and 18.

// ---------------------------------------------------------------------------
// Rows 20-22: inferred slots and bindings
// ---------------------------------------------------------------------------

#[test]
fn row_20_inferred_declarations_take_the_expression_type() {
    let source = entry("sit _ a ← x + y\nvaria _ b ← x * y");
    assert!(kinds(&source).is_empty());
}

#[test]
fn row_21_bindings_are_not_stores() {
    let source = entry("sit lista<u8> xs ← [1, 2]\nitera ex xs sit v {\n    nota v\n}");
    assert!(kinds(&source).is_empty());
}

#[test]
fn row_22_type_pattern_bindings_are_not_stores() {
    // The binding takes the tested member's type; the value matches only if it
    // is a `u8`, so nothing is narrowed.
    let source = "functio probe(u8 ∪ textus u) → vacuum {\n    discerne u {\n        casu u8 sit b { nota b }\n        casu textus sit s { nota s }\n    }\n}\n";
    assert!(kinds(source).is_empty());
}

// ---------------------------------------------------------------------------
// Rows 23-25: unions and type arguments
// ---------------------------------------------------------------------------

#[test]
fn row_23_one_numeric_member_narrows_into_that_member() {
    let source = entry("sit u8 ∪ nihil u ← n");
    let (stores, result) = stores_of(&source);
    assert_eq!(stores.len(), 1);
    assert!(matches!(
        result.types.get(stores[0].target),
        Type::SizedNumeric(_, radix_types::NumericWidth::U8)
    ));
}

#[test]
fn row_24_several_numeric_members_take_no_narrowing() {
    assert_rejected(&entry("sit u8 ∪ i16 u ← n"));
}

#[test]
fn row_25_type_arguments_stay_invariant() {
    assert_rejected(&entry(
        "sit lista<numerus> wide ← [n]\nsit lista<u8> narrow ← wide",
    ));
}

// ---------------------------------------------------------------------------
// Rows 26 and 28: proof-only `∷` and slotless positions
// ---------------------------------------------------------------------------

#[test]
fn row_26_ascription_is_proof_only() {
    assert_rejected(&entry("sit u8 z ← n ∷ u8"));
}

#[test]
fn row_28_slotless_positions_never_narrow() {
    let source = entry("nota x + 10\nsit bivalens c ← x + 10 ≻ 255\nnota n");
    assert!(kinds(&source).is_empty());
}

// ---------------------------------------------------------------------------
// Constants and policy families
// ---------------------------------------------------------------------------

#[test]
fn constant_that_certainly_traps_is_a_compile_error() {
    assert_issue(
        "incipit {\nsit u8 w ← 300\nnota w\n}\n",
        "store_constant_out_of_range",
    );
    assert_issue(
        "incipit {\nsit u8 w ← -1\nnota w\n}\n",
        "store_constant_out_of_range",
    );
}

#[test]
fn folded_constant_expressions_are_checked_at_compile_time() {
    // `10 - 100` is -90 as an exact constant; it does not fit `u8` (ruling 12a).
    assert_issue(
        "incipit {\nsit u8 d ← 10 - 100\nnota d\n}\n",
        "store_constant_out_of_range",
    );
    assert!(kinds("incipit {\nsit u8 d ← 100 + 100\nnota d\n}\n").is_empty());
}

#[test]
fn constant_call_argument_that_traps_is_a_compile_error() {
    assert_issue(
        "functio take(u8 v) → vacuum {\n    nota v\n}\nincipit {\ntake(300)\n}\n",
        "store_constant_out_of_range",
    );
}

#[test]
fn saturating_constant_follows_the_slot_policy() {
    // ruling 21 (amended): `←` is a runtime store, so a constant clamps.
    let source = "incipit {\nsit saturatus<u8> w ← 300\nnota w\n}\n";
    let (stores, result) = stores_of(source);
    assert_eq!(stores.len(), 1);
    assert_eq!(stores[0].site.kind, HirStoreKind::Declaration);
    assert!(matches!(
        result.types.get(stores[0].target),
        Type::SaturatingWord(radix_types::NumericWidth::U8)
    ));
}

#[test]
fn wrapping_constant_follows_the_slot_policy() {
    let source = "incipit {\nsit modulus<u8> w ← 300\nsit modulus<u8> v ← -1\nnota w\nnota v\n}\n";
    let (stores, result) = stores_of(source);
    assert_eq!(stores.len(), 2);
    for store in &stores {
        assert!(matches!(
            result.types.get(store.target),
            Type::ModularWord(radix_types::NumericWidth::U8)
        ));
    }
}

#[test]
fn a_fitting_word_constant_carries_no_node() {
    let source = "incipit {\nsit saturatus<u8> w ← 200\nsit modulus<u8> v ← 7\nnota w\nnota v\n}\n";
    assert!(kinds(source).is_empty());
}

#[test]
fn narrower_saturating_store_clamps_within_its_family() {
    let source =
        "incipit {\nsit saturatus<u16> big ← 1000\nsit saturatus<u8> small ← big\nnota small\n}\n";
    let (stores, result) = stores_of(source);
    assert_eq!(stores.len(), 1);
    assert!(matches!(
        result.types.get(stores[0].target),
        Type::SaturatingWord(radix_types::NumericWidth::U8)
    ));
}

#[test]
fn narrower_wrapping_store_reduces_within_its_family() {
    let source =
        "incipit {\nsit modulus<u32> big ← 1000\nsit modulus<u8> small ← big\nnota small\n}\n";
    let (stores, result) = stores_of(source);
    assert_eq!(stores.len(), 1);
    assert!(matches!(
        result.types.get(stores[0].target),
        Type::ModularWord(radix_types::NumericWidth::U8)
    ));
}

#[test]
fn word_list_sum_is_exact_and_the_store_reduces_it() {
    // The sum adds exactly (it is not a word), so the store into a
    // `modulus<u8>` slot carries the node that reduces the total.
    let source = "incipit {\nsit lista<modulus<u8>> ws ← [200, 100]\nsit modulus<u8> w ← ws.summa()\nnota w\n}\n";
    let (stores, result) = stores_of(source);
    let declarations: Vec<_> = stores
        .iter()
        .filter(|store| store.site.kind == HirStoreKind::Declaration)
        .collect();
    assert_eq!(declarations.len(), 1);
    assert!(matches!(
        result.types.get(declarations[0].target),
        Type::ModularWord(radix_types::NumericWidth::U8)
    ));
}

#[test]
fn saturating_word_list_sum_is_clamped_by_the_store() {
    let source = "incipit {\nsit lista<saturatus<u8>> ss ← [200, 100]\nsit saturatus<u8> s ← ss.summa()\nnota s\n}\n";
    let (stores, result) = stores_of(source);
    let declarations: Vec<_> = stores
        .iter()
        .filter(|store| store.site.kind == HirStoreKind::Declaration)
        .collect();
    assert_eq!(declarations.len(), 1);
    assert!(matches!(
        result.types.get(declarations[0].target),
        Type::SaturatingWord(radix_types::NumericWidth::U8)
    ));
}

#[test]
fn word_list_sum_still_needs_a_conversion_across_families() {
    assert_rejected(
        "incipit {\nsit lista<modulus<u8>> ws ← [200, 100]\nsit saturatus<u8> s ← ws.summa()\nnota s\n}\n",
    );
}

#[test]
fn crossing_policy_families_still_needs_a_conversion() {
    assert_rejected("incipit {\nsit numerus n ← 200\nsit saturatus<u8> w ← n\nnota w\n}\n");
    assert_rejected(
        "incipit {\nsit saturatus<u16> big ← 1000\nsit modulus<u8> small ← big\nnota small\n}\n",
    );
}

#[test]
fn an_open_inference_slot_binds_instead_of_narrowing() {
    // A generic parameter bound by the first argument is not a declared slot:
    // the second argument does not silently narrow into it.
    let source = "functio duo<T>(T a, T b) → T {\n    redde a\n}\n\
                  incipit {\nsit u8 a ← 1\nsit i16 b ← 2\nnota duo(a, b)\n}\n";
    assert_rejected(source);
}

// ---------------------------------------------------------------------------
// The 64-bit cap (rulings 17b, 19a)
// ---------------------------------------------------------------------------

#[test]
fn cap_typed_sum_into_int_is_checked_at_every_store_position() {
    // `i64 + i64` is typed `i64` but its value reaches 2^64 - 2: the store
    // into an `i64` slot is a real check and carries a node at every store
    // position, a plain local (`c`) included.
    let source = "functio id(numerus v) → numerus { redde v }\n\
                  functio add(numerus a, numerus b) → numerus { redde a + b }\n\
                  incipit {\nvaria numerus a ← 1\nsit numerus c ← a + a\nnota id(a + a)\n}\n";
    let mut found = kinds(source);
    found.sort_by_key(|kind| format!("{kind:?}"));
    assert_eq!(
        found,
        vec![
            HirStoreKind::Argument,
            HirStoreKind::Declaration,
            HirStoreKind::Return
        ]
    );
}

#[test]
fn reducta_summa_is_range_typed_at_the_integer_cap() {
    // Ruling 21: an integer sum has an unbounded term count, so its result
    // type reaches the signed or unsigned 64-bit cap before the return store.
    for term in ["i64", "i8", "u8"] {
        let source = format!(
            "functio total(tensor<{term}, [2]> a) → numerus {{\n  redde reducta via summa ex a sit s {{\n    redde s\n  }}\n}}\n"
        );
        assert_eq!(kinds(&source), vec![HirStoreKind::Return], "{term}");
    }
}

#[test]
fn provably_fitting_int_stores_carry_no_node() {
    // `i32 * i32` fits `i64`; a bare `int` copy and a literal need no check.
    let source = "incipit {\nvaria i32 p ← 3\nvaria numerus a ← 1\n\
                  sit numerus q ← p * p\nsit numerus r ← a\nsit numerus s ← 7\n}\n";
    assert!(kinds(source).is_empty());
}

#[test]
fn bitnot_of_u64_is_accepted_into_an_int_slot() {
    // `¬u` keeps its sign at the cap, so it is `i64` and stores into `int`.
    let source = "incipit {\nvaria u64 u ← 5\nsit numerus y ← ¬u\n}\n";
    test_support::analyze_ok_la(source);
}

// ---------------------------------------------------------------------------
// Indexed containers take their element's scalar store rule
// ---------------------------------------------------------------------------

#[test]
fn container_store_narrowing_tensor_element() {
    // `a + a` over `tensor<i8, [2]>` is `tensor<i16, [2]>` (range rule); the
    // store into the same kind and shape is checked at the element.
    let source = "functio total(tensor<i8, [2]> a) → tensor<i8, [2]> {\n\
                  sit tensor<i8, [2]> s ← a + a\n\
                  redde s\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Declaration]);
}

#[test]
fn container_store_narrowing_vector_and_matrix_elements() {
    // The vector and matrix twins follow the same element scalar rule.
    let vector = "functio lane(vector<i8, magnitudo 2> v) → vector<i8, magnitudo 2> {\n\
                  sit vector<i8, magnitudo 2> s ← v + v\n\
                  redde s\n}\n";
    assert_eq!(kinds(vector), vec![HirStoreKind::Declaration]);
    let matrix = "functio cell(matrix<i8, [2, 2]> m) → matrix<i8, [2, 2]> {\n\
                  sit matrix<i8, [2, 2]> s ← m * 2\n\
                  redde s\n}\n";
    assert_eq!(kinds(matrix), vec![HirStoreKind::Declaration]);
}

#[test]
fn container_store_narrowing_equal_element_carries_no_node() {
    // `a + a` over `tensor<i8, [2]>` is already `tensor<i16, [2]>` (range
    // rule), so the store into the equal `i16` slot is total and carries no
    // check node. (`b + b` over `tensor<i16, [2]>` is `i32`, a real narrowing,
    // so it does carry one — see the tensor case above.)
    let equal = "functio same(tensor<i8, [2]> a) → tensor<i16, [2]> {\n\
                 sit tensor<i16, [2]> w ← a + a\n\
                 redde w\n}\n";
    assert!(kinds(equal).is_empty());
    // The identity container copy is likewise total.
    let identity = "functio copy(tensor<i16, [2]> b) → tensor<i16, [2]> {\n\
                    sit tensor<i16, [2]> w ← b\n\
                    redde w\n}\n";
    assert!(kinds(identity).is_empty());
}

#[test]
fn container_store_narrowing_shape_mismatch_is_rejected() {
    // The element rule does not smuggle a shape change past the store.
    let source = "functio wrong(tensor<i8, [2]> a) → tensor<i8, [2]> {\n\
                  sit tensor<i8, [3]> s ← a + a\n\
                  redde s\n}\n";
    assert_rejected(source);
}
