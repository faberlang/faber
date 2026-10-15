//! N7E-2b: a `trapping<W>` float is checked where it is stored (N7e D1, D4).
//!
//! Arithmetic over `trapping<W>` operands is the bare float `W` (an
//! expression's value, not a slot's), so it may be non-finite until a store.
//! Every store of such an expression into a `trapping<W>` slot becomes the
//! implicit store node (`Conversio { implicit: Some(site), .. }`) whose target
//! is the cell. A `trapping<W>` leaf stored into its own cell, an authored
//! `↦` and every `saturating` store carry no node.
//!
//! Position coverage of the twelve `HirStoreKind`s (delivery 4.11). Reached by
//! a float cell and pinned here: Declaration, Assignment, Argument,
//! ParameterDefault, Return, Yield, Throw, Field, Element, TuplePosition and
//! ConversionDefault, plus the variant field and closure return forms.
//! Excluded: AwaitBinding. Its value is what a promise resolves to, a call
//! result, which is the callee's declared type (a leaf cell, or a bare float
//! that needs `↦`), never `trapping` arithmetic; the test pins that no mark
//! and no node appear. Tests assert node presence, kind and target, and
//! diagnostic issues, never prose.

use crate::driver::AnalyzeRequest;
use crate::{Config, Target};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local, walk_program};
use radix_hir::{HirConversioTarget, HirExpression, HirExpressionKind, HirLocal, HirStoreKind, HirStoreSite};
use radix_types::{FloatPolicy, NumericWidth, Type, TypeId};

const ISSUE_MIXING: &str = "policy_float_family_mismatch";

/// One implicit store node found in the analyzed HIR.
struct Store {
    site: HirStoreSite,
    target: TypeId,
}

#[derive(Default)]
struct Collector {
    stores: Vec<Store>,
    /// `await_store` marks on `figendum` / `↢` locals.
    awaits: Vec<HirStoreSite>,
}

impl HirVisitor for Collector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Conversio { target: HirConversioTarget::Type(target), implicit: Some(site), .. } =
            &expr.kind
        {
            self.stores
                .push(Store { site: site.clone(), target: *target });
        }
        walk_expr(self, expr);
    }

    fn visit_local(&mut self, local: &HirLocal) {
        if let Some(site) = &local.await_store {
            self.awaits.push(site.clone());
        }
        walk_local(self, local);
    }
}

fn analyze(config: Config, source: &str) -> (Vec<crate::Diagnostic>, Option<Collected>) {
    let session = crate::driver::Session::new(config);
    match crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => {
            let mut collector = Collector::default();
            walk_program(&mut collector, &unit.hir);
            let diagnostics = unit.diagnostics.clone();
            (diagnostics, Some(Collected { collector, types: unit.types }))
        }
        Err(diagnostics) => (diagnostics, None),
    }
}

struct Collected {
    collector: Collector,
    types: crate::semantic::TypeTable,
}

fn latin_config() -> Config {
    Config::default().with_target(Target::HirRust)
}

fn english_config() -> Config {
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load");
    Config::new().with_locale_pack(pack)
}

fn has_error(diagnostics: &[crate::Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(radix_diagnostics::Diagnostic::is_error)
}

fn has_issue(diagnostics: &[crate::Diagnostic], issue: &str) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.is_error()
            && diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
    })
}

/// Analyze an accepted Latin program: its implicit stores and await marks.
fn collected(source: &str) -> Collected {
    let (diagnostics, collected) = analyze(latin_config(), source);
    assert!(!has_error(&diagnostics), "diagnostics: {diagnostics:?}");
    collected.expect("analyzed")
}

fn kinds(source: &str) -> Vec<HirStoreKind> {
    let collected = collected(source);
    let mut kinds: Vec<HirStoreKind> = collected
        .collector
        .stores
        .iter()
        .map(|store| store.site.kind)
        .collect();
    kinds.extend(collected.collector.awaits.iter().map(|site| site.kind));
    kinds
}

fn assert_rejected_with(source: &str, issue: &str) {
    let (diagnostics, _) = analyze(latin_config(), source);
    assert!(
        has_issue(&diagnostics, issue),
        "expected error issue {issue:?}, got: {diagnostics:?}"
    );
}

fn assert_rejected(source: &str) {
    let (diagnostics, _) = analyze(latin_config(), source);
    assert!(has_error(&diagnostics), "expected a rejection");
}

fn is_trapping_f32(types: &crate::semantic::TypeTable, ty: TypeId) -> bool {
    matches!(types.get(ty), Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F32))
}

/// An entry block with a finite `trapping<f32>` cell `t`, a bare `f32` `x`
/// and a `saturating<f32>` cell `s`.
fn entry(body: &str) -> String {
    format!("incipit {{\nsit exactus<f32> t ← 1.5\nsit f32 x ← 2.5\nsit saturatus<f32> s ← 1.5\n{body}\n}}\n")
}

// ---------------------------------------------------------------------------
// The node: declaration, assignment, and where it is absent
// ---------------------------------------------------------------------------

#[test]
fn policy_float_store_declaration_of_trapping_arithmetic_carries_the_cell() {
    let source = entry("sit exactus<f32> u ← t * t\nnota u");
    let collected = collected(&source);
    let stores = &collected.collector.stores;
    assert_eq!(stores.len(), 1);
    assert_eq!(stores[0].site.kind, HirStoreKind::Declaration);
    assert!(stores[0].site.slot.is_some(), "the declared name rides the node");
    assert!(stores[0].site.inferred_from.is_none());
    assert!(is_trapping_f32(&collected.types, stores[0].target));
}

#[test]
fn policy_float_store_every_arithmetic_operator_is_a_store() {
    for expr in [
        "t + t",
        "t - t",
        "t * t",
        "t / t",
        "t % t",
        "t ÷ t",
        "-(t * t)",
        "t * t + t",
        "t + 1",
        "2 * t",
        "(t + 1.5) * 2.5",
        "t + t * t",
    ] {
        let source = entry(&format!("sit exactus<f32> u ← {expr}\nnota u"));
        assert_eq!(kinds(&source), vec![HirStoreKind::Declaration], "{expr}");
    }
}

#[test]
fn policy_float_store_a_cell_leaf_in_its_own_cell_carries_no_node() {
    let source = entry(
        "sit exactus<f32> v ← t\nsit exactus<f32> w ← -t\nsit exactus<f32> c ← 1.5\n\
         varia exactus<f32> r ← t\nr ← t\nr ← 2.5\nnota v\nnota w\nnota c\nnota r",
    );
    assert!(kinds(&source).is_empty());
}

#[test]
fn policy_float_store_a_bare_float_slot_is_not_checked() {
    // The container decides: a bare `f32` slot holds whatever IEEE gives.
    let source = entry("sit f32 y ← t * t\nvaria f32 z ← x\nz ← t * t\nnota y\nnota z");
    assert!(kinds(&source).is_empty());
}

#[test]
fn policy_float_store_an_authored_conversion_carries_no_node() {
    let source = entry("sit exactus<f32> v ← x ↦ exactus<f32>\nnota v");
    assert!(kinds(&source).is_empty());
}

#[test]
fn policy_float_store_a_saturating_store_never_gets_a_node() {
    let source = entry(
        "sit saturatus<f32> a ← s * s\nvaria saturatus<f32> b ← s\nb ← s + s\n\
         sit saturatus<f32> c ← s\nnota a\nnota b\nnota c",
    );
    assert!(kinds(&source).is_empty());
    // Trapping arithmetic never lands in a saturating slot without `↦`.
    assert_rejected(&entry("sit saturatus<f32> bad ← t * t\nnota bad"));
}

#[test]
fn policy_float_store_assignment_is_a_store() {
    let source = entry("varia exactus<f32> v ← t\nv ← t * t\nv ← v + 1\nnota v");
    assert_eq!(kinds(&source), vec![HirStoreKind::Assignment, HirStoreKind::Assignment]);
}

// ---------------------------------------------------------------------------
// Inferred slots keep the cell
// ---------------------------------------------------------------------------

#[test]
fn policy_float_store_an_inferred_slot_infers_the_cell() {
    let source = entry("sit y ← t * t\nsit z ← t + 1\nnota y\nnota z");
    let collected = collected(&source);
    let stores = &collected.collector.stores;
    assert_eq!(stores.len(), 2);
    for store in stores {
        assert_eq!(store.site.kind, HirStoreKind::Declaration);
        assert!(is_trapping_f32(&collected.types, store.target));
    }
}

#[test]
fn policy_float_store_an_inferred_slot_from_a_leaf_or_a_bare_float_has_no_node() {
    let source = entry("sit a ← t\nsit b ← -t\nsit c ← x * x\nnota a\nnota b\nnota c");
    assert!(kinds(&source).is_empty());
}

#[test]
fn policy_float_store_an_inferred_slot_is_the_cell_for_later_stores() {
    // `y` is `trapping<f32>`: a later bare `f32` store into it needs `↦`.
    assert_rejected(&entry("varia _ y ← t * t\ny ← x\nnota y"));
    let source = entry("varia _ y ← t * t\ny ← t * 2\nnota y");
    let kinds = kinds(&source);
    assert_eq!(kinds, vec![HirStoreKind::Declaration, HirStoreKind::Assignment]);
}

// ---------------------------------------------------------------------------
// The mixing rule sees through nesting
// ---------------------------------------------------------------------------

#[test]
fn policy_float_store_a_bare_float_beside_trapping_arithmetic_is_the_mixing_issue() {
    for expr in [
        "(t * t) + x",
        "x + (t * t)",
        "(t * t) * x",
        "x - (t + t)",
        "(t * t) / x",
        "-(t * t) + x",
        "((t * t) + t) + x",
        "(t * t) ÷ x",
    ] {
        assert_rejected_with(&entry(&format!("sit f32 q ← {expr}\nnota q")), ISSUE_MIXING);
    }
}

#[test]
fn policy_float_store_trapping_arithmetic_beside_another_cell_or_width_is_the_mixing_issue() {
    assert_rejected_with(
        &entry("sit exactus<f64> w ← 1.5\nsit f64 q ← (t * t) + (w * w)\nnota q"),
        ISSUE_MIXING,
    );
    assert_rejected_with(&entry("sit f32 q ← (t * t) + s\nnota q"), ISSUE_MIXING);
    assert_rejected_with(&entry("sit f32 q ← (s * s) + (t * t)\nnota q"), ISSUE_MIXING);
}

#[test]
fn policy_float_store_untyped_constants_and_same_cell_operands_mix_freely() {
    let source = entry(
        "sit exactus<f32> u ← (t * t) + 1.5\nsit exactus<f32> v ← (t * t) + (t - 2)\n\
         sit exactus<f32> w ← 2 * (t * t) + 1\nnota u\nnota v\nnota w",
    );
    assert_eq!(kinds(&source).len(), 3);
}

// ---------------------------------------------------------------------------
// An unstored expression is bare IEEE: nothing is checked, nothing is typed
// as the cell
// ---------------------------------------------------------------------------

#[test]
fn policy_float_store_an_unstored_expression_has_no_node() {
    let source = entry("nota t * t\nsit bivalens c ← t * t ≻ 1.0\nnota c");
    assert!(kinds(&source).is_empty());
}

#[test]
fn policy_float_store_trapping_arithmetic_is_the_bare_float() {
    // It lands in a bare `f32` slot unchanged, and does not land in a `f64`
    // cell or a `saturating` cell.
    assert!(kinds(&entry("sit f32 y ← t * t\nnota y")).is_empty());
    assert_rejected(&entry("sit exactus<f64> y ← t * t\nnota y"));
}

// ---------------------------------------------------------------------------
// The twelve store position kinds (HirStoreKind)
// ---------------------------------------------------------------------------

const TAKE: &str = "functio take(exactus<f32> c) → vacuum {\n    nota c\n}\n";

#[test]
fn policy_float_store_position_argument() {
    let source = format!("{TAKE}incipit {{\nsit exactus<f32> t ← 1.5\ntake(t * t)\ntake(t)\n}}\n");
    assert_eq!(kinds(&source), vec![HirStoreKind::Argument]);
}

#[test]
fn policy_float_store_position_method_argument() {
    let source = entry("varia lista<exactus<f32>> xs ← [t]\nxs.appende(t * t)\nxs.appende(t)\nnota xs");
    assert_eq!(kinds(&source), vec![HirStoreKind::Argument]);
}

#[test]
fn policy_float_store_position_parameter_default() {
    // A default reads module constants (`=`), here `trapping<f32>` cells.
    let source = "fixum exactus<f32> K = 1.5\nfunctio f(exactus<f32> v vel K * K) → exactus<f32> {\n    redde v\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::ParameterDefault]);
    let leaf = "fixum exactus<f32> K = 1.5\nfunctio f(exactus<f32> v vel K) → exactus<f32> {\n    redde v\n}\n";
    assert!(kinds(leaf).is_empty());
}

#[test]
fn policy_float_store_position_return() {
    let source = "functio sq(exactus<f32> v) → exactus<f32> {\n    redde v * v\n}\n\
                  functio same(exactus<f32> v) → exactus<f32> {\n    redde v\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Return]);
}

#[test]
fn policy_float_store_position_closure_return() {
    let source = entry(
        "sit (exactus<f32>) → exactus<f32> f ← clausura exactus<f32> v → exactus<f32> { redde v * v }\nnota f(t)",
    );
    assert_eq!(kinds(&source), vec![HirStoreKind::Return]);
}

#[test]
fn policy_float_store_position_yield() {
    let source = "functio gignit(exactus<f32> v) fiunt → exactus<f32> {\n    cede v * v\n    cede v\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Yield]);
}

#[test]
fn policy_float_store_position_throw() {
    let source = "functio iacit(exactus<f32> v) → numerus ⇥ exactus<f32> {\n    iace v * v\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Throw]);
}

#[test]
fn policy_float_store_position_field() {
    let source = "genus Arca {\n    fixum exactus<f32> a\n}\n\
                  incipit {\nsit exactus<f32> t ← 1.5\nsit Arca b ← Arca { a = t * t }\nsit Arca c ← Arca { a = t }\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Field]);
}

#[test]
fn policy_float_store_position_variant_field() {
    let source = "discretio Forma {\n    Plena { exactus<f32> res }\n}\n\
                  incipit {\nsit exactus<f32> t ← 1.5\nsit Forma f ← finge Plena { res = t * t }\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Field]);
}

#[test]
fn policy_float_store_position_element() {
    let source = entry("sit lista<exactus<f32>> xs ← [t * t, t]\nnota xs");
    assert_eq!(kinds(&source), vec![HirStoreKind::Element]);
}

#[test]
fn policy_float_store_position_tuple() {
    let source =
        entry("sit iuncta<exactus<f32>, exactus<f32>> p ← iuncta<exactus<f32>, exactus<f32>>[t * t, t]\nnota p");
    assert_eq!(kinds(&source), vec![HirStoreKind::TuplePosition]);
}

#[test]
fn policy_float_store_position_conversion_default() {
    let source = entry("sit numerus n ← 5\nsit exactus<f32> r ← n ↦ exactus<f32> ⊥ (t * t)\nnota r");
    assert_eq!(kinds(&source), vec![HirStoreKind::ConversionDefault]);
}

#[test]
fn policy_float_store_position_await_binding_never_carries_trapping_arithmetic() {
    // The awaited value is the promise's inner type: a call result is the
    // callee's declared type (a leaf cell, or a bare float), never trapping
    // arithmetic. A leaf cell needs no mark; a bare float needs an authored
    // `↦`, as for any other store (D4).
    let leaf = "functio fetch() fiet → exactus<f32> {\n    redde 1.5\n}\n\
                functio caller() fiet → vacuum {\n    sit exactus<f32> v ↢ fetch()\n    nota v\n}\n";
    assert!(kinds(leaf).is_empty());
    let bare = "functio fetch() fiet → f32 {\n    redde 1.5\n}\n\
                functio caller() fiet → vacuum {\n    sit exactus<f32> v ↢ fetch()\n    nota v\n}\n";
    assert_rejected(bare);
}

// ---------------------------------------------------------------------------
// Other slot shapes
// ---------------------------------------------------------------------------

#[test]
fn policy_float_store_a_single_cell_member_of_a_union_slot_is_checked() {
    let source = entry("sit exactus<f32> ∪ nihil u ← t * t\nnota u");
    let collected = collected(&source);
    let stores = &collected.collector.stores;
    assert_eq!(stores.len(), 1);
    assert!(is_trapping_f32(&collected.types, stores[0].target));
}

#[test]
fn policy_float_store_element_and_field_assignments_are_stores() {
    let source = "genus Arca {\n    varia exactus<f32> a\n}\n\
                  incipit {\nsit exactus<f32> t ← 1.5\nvaria Arca b ← Arca { a = t }\nb.a ← t * t\n\
                  varia lista<exactus<f32>> xs ← [t]\nxs[0] ← t + t\nnota b\nnota xs\n}\n";
    assert_eq!(kinds(source), vec![HirStoreKind::Assignment, HirStoreKind::Assignment]);
}

// ---------------------------------------------------------------------------
// The exemplum of delivery 4.11 (en): faber check prints no error
// ---------------------------------------------------------------------------

#[test]
fn policy_float_store_the_exemplum_source_checks_clean() {
    let source = r"
main {
    let f32 m ← 3.4028235e38
    let saturating<f32> s ← m ↦ saturating<f32>
    print s * s
    let saturating<f32> p ← s * s
    print p
    let saturating<f32> zero ← 0.0
    print zero / zero
    let inf huge ← 2 ⇐ 2000
    print huge ↦ saturating<f64>
    let f64 wide ← 1.0e300
    print wide ↦ saturating<f32>
    let trapping<f32> t ← m ↦ trapping<f32>
    print t * t
    let trapping<f32> u ← t * t
    print u
}
";
    let (diagnostics, collected) = analyze(english_config(), source);
    assert!(!has_error(&diagnostics), "diagnostics: {diagnostics:?}");
    // Exactly one implicit store of a cell: `u ← t * t` (the `p ← s * s`
    // saturating store carries none).
    let collected = collected.expect("analyzed");
    let stores = &collected.collector.stores;
    let cells: Vec<&Store> = stores
        .iter()
        .filter(|store| is_trapping_f32(&collected.types, store.target))
        .collect();
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].site.kind, HirStoreKind::Declaration);
    assert!(
        !stores
            .iter()
            .any(|store| matches!(collected.types.get(store.target), Type::PolicyFloat(FloatPolicy::Saturating, _))),
        "a saturating store never gets a node"
    );
}
