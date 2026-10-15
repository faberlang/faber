//! Union-variant pattern binding tests (uvf-u1).
//!
//! Covers the same-module union `match` with named arms + payload bindings
//! (defect-sprint G2b) and the imported-union consumer match (SEM001). The
//! fixtures here started as the uvf-u1 probe: they pinned the post-uvn
//! behavior before the product change. Probe finding — same-module union
//! match already checked green post-uvn (the goal's G2b claim was a masking
//! artifact); the imported `discerne` path pattern failed at lowering
//! (`lower_path_pattern` synthesized a fresh DefId for the terminal segment
//! instead of resolving the per-union namespace), surfacing as
//! `unknown_variant` in the pattern typecheck pass.

use std::collections::BTreeMap;

use crate::test_support;

use crate::Primitive;
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceEnumVariant, InterfaceNominalExport,
    InterfaceStructField, InterfaceTypeSnapshot,
};

// ---------------------------------------------------------------------------
// Probe P1: same-module union match, named arms + payload bindings
// ---------------------------------------------------------------------------

#[test]
fn same_module_union_match_binds_named_arms_and_payload() {
    // uvf-u1 done_when (b): same-module union `match` with named arms +
    // payload bindings checks green. The scrutinee is a union value; the arm
    // binds the payload field and the arm body uses it.
    test_support::assert_accepts_la(
        r#"discretio Eventum {
    Ictus { numerus vis },
    Sonitus
}

functio describe(Eventum e) → textus {
    discerne e {
        casu Eventum.Ictus fixum vis { redde vis ↦ textus }
        casu Eventum.Sonitus { redde "sonitus" }
    }
}

incipit {
    sit Eventum a ← finge Ictus { vis = 3 } ∷ Eventum
    sit Eventum b ← Eventum.Sonitus
    nota describe(a) + describe(b)
}"#,
    );
}

#[test]
fn same_module_union_match_missing_variant_still_errors() {
    // uvf-u1 done_when (d): a genuinely-missing variant name still errors.
    test_support::assert_rejects_with_issue_la(
        r#"discretio Eventum { Ictus, Sonitus }

functio describe(Eventum e) → textus {
    discerne e {
        casu Eventum.Perculsus { redde "perculsus" }
        casu Eventum.Sonitus { redde "sonitus" }
    }
}

incipit {
    sit Eventum e ← Eventum.Sonitus
    nota describe(e)
}"#,
        "unknown_variant",
    );
}

// ---------------------------------------------------------------------------
// Probe P2: imported-union consumer match
// ---------------------------------------------------------------------------

/// Install a producer-shaped file interface: a public union exports as an
/// `Enum`-kind nominal carrying its variant metadata, exactly as the CLI
/// export assembly records a `discretio` today.
fn token_union_interface() -> FileInterface {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "TokenKind".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            params: Vec::new(),
            name: "TokenKind".to_owned(),
            methods: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
            variants: vec![
                InterfaceEnumVariant {
                    name: "Palabra".to_owned(),
                    fields: Vec::new(),
                },
                InterfaceEnumVariant {
                    name: "Numerus".to_owned(),
                    fields: vec![InterfaceStructField {
                        name: "code".to_owned(),
                        ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                        optional: false,
                        required: true,
                        default: None,
                        modifier: None,
                        visibility: radix_syntax::MemberVisibility::Publica,
                    }],
                },
            ],
        }),
    });
    interface
}

fn analyze_consumer(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_with_import_contract_la(
        source,
        BTreeMap::from([(
            "token".to_owned(),
            BTreeMap::from([(
                "TokenKind".to_owned(),
                crate::passes::visibility_annotation::VisibilityTier::Publica,
            )]),
        )]),
        BTreeMap::from([("token".to_owned(), token_union_interface())]),
    )
}

/// Run the consumer analysis and return the flat diagnostic list (errors and
/// warnings), matching `driver::analyze_source_la`-style test consumers.
fn analyze_consumer_diagnostics(source: &str) -> Vec<crate::SemanticError> {
    analyze_consumer(source).diagnostics.clone()
}

#[test]
fn imported_union_consumer_match_via_discerne_path_pattern() {
    // uvf-u1 done_when (c): a consumer module importing a union can `match`
    // its variants with no `unknown_variant`.
    let source = r#"importa ex "./token" token

functio kind_name(token.TokenKind k) → textus {
    discerne k {
        casu token.TokenKind.Palabra { redde "palabra" }
        casu token.TokenKind.Numerus fixum code { redde code ↦ textus }
    }
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected imported-union consumer match to check green, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_consumer_match_via_casu_value() {
    // The `elige`-style `casu` value surface (rivus `case token.TokenKind.X`).
    let source = r#"importa ex "./token" token

functio is_palabra(token.TokenKind k) → bivalens {
    elige k {
        casu token.TokenKind.Palabra { redde verum }
        ceterum { redde falsum }
    }
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected imported-union casu value match to check green, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_missing_variant_still_errors() {
    // uvf-u1 done_when (d) on the imported surface: a genuinely-missing
    // variant name still raises `unknown_variant`.
    let source = r#"importa ex "./token" token

functio kind_name(token.TokenKind k) → textus {
    discerne k {
        casu token.TokenKind.Perculsus { redde "perculsus" }
    }
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error() && d.issue() == Some("unknown_variant")),
        "expected unknown_variant for a missing imported variant, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_payload_arity_still_checked() {
    // Payload-arity checking is unchanged: matching a payload-bearing variant
    // without bindings raises the variant-pattern arity mismatch, exactly as
    // the enum surface does.
    let source = r#"importa ex "./token" token

functio kind_name(token.TokenKind k) → textus {
    discerne k {
        casu token.TokenKind.Palabra { redde "palabra" }
        casu token.TokenKind.Numerus { redde "numerus" }
    }
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error() && d.issue() == Some("variant_pattern_arity_mismatch")),
        "expected payload arity mismatch for an unbound payload arm, got: {diagnostics:?}"
    );
}

#[test]
fn enum_match_behavior_unchanged() {
    // uvf-u1 done_when (e): existing enum match behavior is a regression
    // surface — named arms and missing-variant errors behave as before on an
    // `ordo`.
    test_support::assert_accepts_la(
        r#"ordo Colores {
    Rubrum,
    Viride
}

functio describe(Colores c) → textus {
    discerne c {
        casu Colores.Rubrum { redde "rubrum" }
        casu Colores.Viride { redde "viride" }
    }
}"#,
    );
    test_support::assert_rejects_with_issue_la(
        r#"ordo Colores { Rubrum, Viride }

functio describe(Colores c) → textus {
    discerne c {
        casu Colores.Aureus { redde "aureus" }
        casu Colores.Rubrum { redde "rubrum" }
    }
}"#,
        "unknown_variant",
    );
}

// ---------------------------------------------------------------------------
// Probe P3: qualified pattern prefixes (CG2-R07 mirror)
// ---------------------------------------------------------------------------

#[test]
fn rejects_qualified_pattern_with_unknown_prefix() {
    // CG2-R07 mirror: a leading path segment that is not a real qualifier
    // must reject — `casu missing.A.X { }` must never silently bind the
    // local `A.X` as a pattern.
    test_support::assert_rejects_with_issue_la(
        r#"discretio A { X }

functio f(A a) → textus {
    discerne a {
        casu missing.A.X { redde "x" }
    }
}

incipit {
    sit A a ← A.X
    nota f(a)
}"#,
        "unknown_variant",
    );
}

#[test]
fn rejects_qualified_pattern_with_non_namespace_prefix() {
    // CG2-R07 mirror: a value binding in prefix position is not a namespace —
    // the path must reject rather than resolve the penultimate local union.
    test_support::assert_rejects_with_issue_la(
        r#"discretio A { X }

functio f(A a) → textus {
    sit numerus missing ← 1
    discerne a {
        casu missing.A.X { redde "x" }
    }
    redde missing ↦ textus
}"#,
        "unknown_variant",
    );
}

#[test]
fn allows_exact_local_qualified_pattern() {
    // CG2-R07 mirror regression pin: the exact local qualifier `casu A.X { }`
    // still resolves through the local union's parent.
    test_support::assert_accepts_la(
        r#"discretio A { X }

functio f(A a) → textus {
    discerne a {
        casu A.X { redde "x" }
    }
}

incipit {
    sit A a ← A.X
    nota f(a)
}"#,
    );
}

#[test]
fn imported_union_full_path_with_local_shadow_binds_imported_identity() {
    // CG2-R07 mirror: the leading path segment must resolve as a real
    // qualifier before the penultimate segment's local enum/union name
    // matches — a local union named `TokenKind` must never capture the
    // qualified pattern `casu token.TokenKind.Numerus fixum code`, which
    // binds the imported variant identity through the `token` namespace.
    let source = r#"importa ex "./token" token

discretio TokenKind {
    Numerus { numerus code },
    Palabra
}

functio kind_name(token.TokenKind k) → textus {
    discerne k {
        casu token.TokenKind.Numerus fixum code { redde code ↦ textus }
        ceterum { redde "palabra" }
    }
}"#;
    let mut unit = analyze_consumer(source);
    assert!(
        unit.is_ok(),
        "qualified pattern with a local shadow union should analyze green: {:?}",
        unit.diagnostics
    );

    let token = unit.interner.intern("token");
    let token_kind = unit.interner.intern("TokenKind");
    let numerus = unit.interner.intern("Numerus");
    let imported_numerus = unit
        .resolver
        .namespace_file_type_variant(token, token_kind, numerus)
        .expect("token.TokenKind.Numerus must be a known imported variant")
        .0;
    let local_numerus = unit
        .resolver
        .variant_child(
            unit.resolver.lookup(token_kind).expect("local union def"),
            numerus,
        )
        .expect("local TokenKind.Numerus def");
    assert_ne!(
        imported_numerus, local_numerus,
        "local and imported variants must be distinct DefIds"
    );

    let mut collector = PatternVariantCollector {
        def_ids: std::collections::HashSet::new(),
    };
    radix_hir::visit::walk_program(&mut collector, unit.hir.as_ref().expect("hir"));
    let variant_defs: std::collections::HashSet<_> = collector
        .def_ids
        .into_iter()
        .filter(|def_id| {
            unit.resolver
                .get_symbol(*def_id)
                .is_some_and(|symbol| symbol.kind == crate::SymbolKind::Variant)
        })
        .collect();
    assert_eq!(
        variant_defs,
        std::collections::HashSet::from([imported_numerus]),
        "the qualified pattern must bind the imported identity, never the local shadow: {variant_defs:?}"
    );
}

/// Collect every variant `DefId` the HIR binds through pattern variants — the
/// qualified-identity proof surface for the shadow fixture.
struct PatternVariantCollector {
    def_ids: std::collections::HashSet<radix_hir::DefId>,
}

impl radix_hir::visit::HirVisitor for PatternVariantCollector {
    fn visit_pattern(&mut self, pattern: &radix_hir::HirPattern) {
        if let radix_hir::HirPattern::Variant(def_id, _) = pattern {
            self.def_ids.insert(*def_id);
        }
        radix_hir::visit::walk_pattern(self, pattern);
    }
}

// ---------------------------------------------------------------------------
// Probe P4: elige casu-value qualifiers (CG2-R07 expression-position mirror)
// ---------------------------------------------------------------------------

#[test]
fn rejects_elige_casu_value_with_unknown_prefix() {
    // CG2-R07 expression-position mirror: an `elige` casu value with leading
    // path segments must resolve the leading path as a real qualifier — a
    // fully-unknown prefix like `casu missing.A.X { }` must reject as an
    // unknown variant, never silently bind the local `A.X`.
    test_support::assert_rejects_with_issue_la(
        r#"discretio A { X }

functio f(A a) → textus {
    elige a {
        casu missing.A.X { redde "x" }
        ceterum { redde "?" }
    }
}

incipit {
    sit A a ← A.X
    nota f(a)
}"#,
        "unknown_variant",
    );
}

#[test]
fn rejects_elige_casu_value_with_non_namespace_prefix() {
    // CG2-R07 expression-position mirror: a value binding in prefix position
    // is not a namespace — the casu value must reject rather than silently
    // bind the local union's variant through the bare-name fallback.
    test_support::assert_rejects_with_issue_la(
        r#"discretio A { X }

functio f(A a) → textus {
    sit numerus missing ← 1
    elige a {
        casu missing.A.X { redde "x" }
        ceterum { redde "?" }
    }
    redde missing ↦ textus
}

incipit {
    sit A a ← A.X
    nota f(a)
}"#,
        "unknown_variant",
    );
}

#[test]
fn allows_exact_local_qualified_elige_casu_value() {
    // CG2-R07 expression-position regression pin: the exact local qualifier
    // `casu A.X { }` still resolves through the local union's parent.
    test_support::assert_accepts_la(
        r#"discretio A { X }

functio f(A a) → textus {
    elige a {
        casu A.X { redde "x" }
        ceterum { redde "?" }
    }
}

incipit {
    sit A a ← A.X
    nota f(a)
}"#,
    );
}

#[test]
fn allows_real_imported_qualified_elige_casu_value() {
    // CG2-R07 expression-position pin: a casu value qualified through a real
    // imported namespace (`token.TokenKind.Palabra`) checks green.
    let source = r#"importa ex "./token" token

functio is_palabra(token.TokenKind k) → bivalens {
    elige k {
        casu token.TokenKind.Palabra { redde verum }
        ceterum { redde falsum }
    }
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected imported-union elige casu value to check green, got: {diagnostics:?}"
    );
}

#[test]
fn elige_casu_reject_raises_resolve_stage_unknown_variant() {
    // CG2-R07 expression-position mirror regression pin: the elige reject
    // must fire in the resolve pass (SEM001.unknown_variant), exactly like
    // the pattern-position mirror — not only at lowering (SEM008). A merge
    // fixup (census-schema conflict resolution) dropped the resolve-side
    // check while the lowering check survived, so the lowering error alone
    // masked the regression and `missing.A.X` was silently resolved as an
    // expression through the bare-name fallback.
    let diagnostics = test_support::compile_and_get_diagnostics_la(
        r#"discretio A { X }

functio f(A a) → textus {
    sit numerus missing ← 1
    elige a {
        casu missing.A.X { redde "x" }
        ceterum { redde "?" }
    }
    redde missing ↦ textus
}

incipit {
    sit A a ← A.X
    nota f(a)
}"#,
    );
    assert!(
        diagnostics.iter().any(|d| d.is_error()
            && crate::semantic_spec(d.kind).code == "SEM001"
            && d.issue() == Some("unknown_variant")),
        "expected resolve-stage unknown_variant (SEM001) for an elige casu with a non-namespace prefix, got: {diagnostics:?}"
    );
}

// X7-b: `aut` alternatives are one arm and therefore must expose one body-safe
// binding environment.
#[test]
fn or_pattern_accepts_payload_free_and_identical_payload_bindings() {
    test_support::assert_accepts_la(
        r#"discretio Eventum {
    Ictus { numerus vis },
    Tactus { numerus vis },
    Sonitus,
    Quietus
}

functio describe(Eventum e) → textus {
    discerne e {
        casu Eventum.Ictus fixum vis aut Eventum.Tactus fixum vis { redde vis ↦ textus }
        casu Eventum.Sonitus { redde "sonitus" }
        casu Eventum.Quietus { redde "quietus" }
    }
}

functio quiet(Eventum e) → vacuum {
    discerne e {
        casu Eventum.Sonitus aut Eventum.Quietus { tacet }
        casu Eventum.Ictus fixum vis { tacet }
        casu Eventum.Tactus fixum vis { tacet }
    }
}"#,
    );
}

#[test]
fn or_pattern_rejects_mismatched_binding_names() {
    test_support::assert_rejects_with_issue_la(
        r#"discretio Eventum { Ictus { numerus vis }, Tactus { numerus vis } }

functio describe(Eventum e) → textus {
    discerne e {
        casu Eventum.Ictus fixum left aut Eventum.Tactus fixum right { redde "x" }
    }
}"#,
        "or_pattern_binding_mismatch",
    );
}

#[test]
fn or_pattern_rejects_mismatched_binding_types() {
    test_support::assert_rejects_with_issue_la(
        r#"discretio Eventum { Ictus { numerus vis }, Verbum { textus vis } }

functio describe(Eventum e) → textus {
    discerne e {
        casu Eventum.Ictus fixum vis aut Eventum.Verbum fixum vis { redde "x" }
    }
}"#,
        "or_pattern_binding_mismatch",
    );
}
