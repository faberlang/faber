//! Union-variant construction binding tests (uvf-u3).
//!
//! Covers qualified variant construction of an imported union
//! (`finge pkg.Bonum { … } ∷ pkg.Exitus`) and the union-qualified same-module
//! form (`finge Eventum.Ictus { … } ∷ Eventum`), plus the same-module
//! regression surface. The rivus-shaped fixture replaces a
//! `token_kind_*()` constructor with a qualified reference (`done_when` d).

use std::collections::BTreeMap;

use crate::test_support;

use crate::Primitive;
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceEnumVariant, InterfaceNominalExport,
    InterfaceStructField, InterfaceTypeSnapshot,
};

// ---------------------------------------------------------------------------
// Probe P1: same-module qualified construction
// ---------------------------------------------------------------------------

#[test]
fn same_module_qualified_finge_construction_checks_green() {
    // uvf-u3 done_when (c) regression + the union-qualified same-module form:
    // `finge Eventum.Ictus { … } ∷ Eventum` binds to the same union's variant
    // exactly like the bare `finge Ictus { … } ∷ Eventum` construction.
    test_support::assert_accepts(
        r"discretio Eventum {
    Ictus { numerus vis },
    Sonitus
}

incipit {
    sit Eventum a ← finge Ictus { vis = 3 } ∷ Eventum
    sit Eventum b ← finge Eventum.Ictus { vis = 4 } ∷ Eventum
    sit Eventum c ← finge Eventum.Sonitus { } ∷ Eventum
}",
    );
}

#[test]
fn same_module_missing_qualified_variant_still_errors() {
    // A qualified construction naming a variant the union does not define
    // keeps failing closed (never a silent identity).
    test_support::assert_rejects_with_issue(
        r"discretio Eventum { Ictus, Sonitus }

incipit {
    sit Eventum a ← finge Eventum.Perculsus { } ∷ Eventum
    nota a
}",
        "unknown_variant",
    );
}

// ---------------------------------------------------------------------------
// Probe P2: imported-union qualified construction
// ---------------------------------------------------------------------------

/// Install a producer-shaped file interface: a public union exports as an
/// `Enum`-kind nominal carrying its variant metadata, exactly as the CLI
/// export assembly records a `discretio` today (same shape as uvf-u1).
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
    crate::test_support::analyze_source_with_import_contract(
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

/// Run the consumer analysis and return the flat diagnostic list.
fn analyze_consumer_diagnostics(source: &str) -> Vec<crate::SemanticError> {
    analyze_consumer(source).diagnostics.clone()
}

#[test]
fn imported_union_qualified_finge_construction_checks_green() {
    // uvf-u3 done_when (b): `finge token.Palabra { … } ∷ token.TokenKind`
    // (module-alias qualified variant + qualified cast) parses and binds to
    // the imported variant identity.
    let source = r#"importa ex "./token" token

functio construct() → token.TokenKind {
    redde finge token.Palabra { } ∷ token.TokenKind
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected imported-union qualified construction to check green, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_full_path_finge_construction_checks_green() {
    // The three-segment form — `finge token.TokenKind.Palabra { … }` — names
    // the imported union explicitly as the penultimate segment, resolving the
    // variant through the imported enum's parent link.
    let source = r#"importa ex "./token" token

functio construct() → token.TokenKind {
    redde finge token.TokenKind.Numerus { code = 7 } ∷ token.TokenKind
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected imported-union full-path construction to check green, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_full_path_with_local_shadow_binds_imported_identity() {
    // CG2-R07: the leading path segment must resolve as a real qualifier
    // before the penultimate segment's local enum/union name matches — a
    // local union named `TokenKind` must never capture the qualified
    // construction `finge token.TokenKind.Numerus { … }`, which binds the
    // imported variant identity through the `token` namespace.
    let source = r#"importa ex "./token" token

discretio TokenKind {
    Numerus { numerus code },
    Palabra
}

functio construct() → token.TokenKind {
    redde finge token.TokenKind.Numerus { code = 7 } ∷ token.TokenKind
}"#;
    let mut unit = analyze_consumer(source);
    assert!(
        unit.is_ok(),
        "qualified construction with a local shadow union should analyze green: {:?}",
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

    let mut collector = VariantPathCollector {
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
        "the qualified construction must bind the imported identity, never the local shadow: {variant_defs:?}"
    );
}

#[test]
fn imported_union_qualified_finge_with_payload_checks_green() {
    // uvf-u3 done_when (d), rivus shape: a payload-bearing variant constructs
    // through the import alias, replacing a `token_kind_*()` constructor.
    let source = r#"importa ex "./token" token

functio token_kind_numerus(numerus code) → token.TokenKind {
    redde finge token.Numerus { code = code } ∷ token.TokenKind
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected rivus-shaped qualified payload construction to check green, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_missing_qualified_variant_still_errors() {
    // A qualified construction naming a variant the imported union does not
    // export keeps failing closed.
    let source = r#"importa ex "./token" token

functio construct() → token.TokenKind {
    redde finge token.Perculsus { } ∷ token.TokenKind
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    assert!(
        diagnostics.iter().any(|d| {
            d.is_error() && matches!(d.issue(), Some("unknown_variant" | "unknown_finge_variant"))
        }),
        "expected an unknown-variant error for a missing imported variant, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_qualified_cast_unknown_union_still_errors() {
    // The qualified cast itself must resolve: a union the module does not
    // export cannot be the construction target.
    let source = r#"importa ex "./token" token

functio construct() → token.TokenKind {
    redde finge token.Palabra { } ∷ token.Exitus
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    assert!(
        diagnostics.iter().any(crate::SemanticError::is_error),
        "expected an error for an unknown qualified cast target, got: {diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// Probe P3: two-import collision fixture (CTO UVN-1)
// ---------------------------------------------------------------------------

/// Two producer-shaped file interfaces: module `alpha` exports union `A` with
/// variant `X`; module `beta` exports union `B` with a distinct variant also
/// named `X` plus a `Missing` variant. The shared terminal name `X` is the
/// collision oracle — qualified paths must resolve through their own imported
/// parent before any bare-variant fallback, or the two identities would
/// collapse into a false `variant_ambiguous`. `beta.B.Missing` doubles as the
/// bare-resolvable oracle for the fail-closed direction: `alpha.A.Missing`
/// must never bind through the sibling union.
fn two_import_interfaces() -> BTreeMap<String, FileInterface> {
    let mut alpha = FileInterface::new();
    alpha.insert(FileExport {
        name: "A".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            params: Vec::new(),
            name: "A".to_owned(),
            methods: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
            variants: vec![InterfaceEnumVariant {
                name: "X".to_owned(),
                fields: Vec::new(),
            }],
        }),
    });
    let mut beta = FileInterface::new();
    beta.insert(FileExport {
        name: "B".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            params: Vec::new(),
            name: "B".to_owned(),
            methods: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
            variants: vec![
                InterfaceEnumVariant {
                    name: "X".to_owned(),
                    fields: Vec::new(),
                },
                InterfaceEnumVariant {
                    name: "Missing".to_owned(),
                    fields: Vec::new(),
                },
            ],
        }),
    });
    BTreeMap::from([("alpha".to_owned(), alpha), ("beta".to_owned(), beta)])
}

fn analyze_two_import_consumer(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_with_import_contract(
        source,
        BTreeMap::from([
            (
                "alpha".to_owned(),
                BTreeMap::from([(
                    "A".to_owned(),
                    crate::passes::visibility_annotation::VisibilityTier::Publica,
                )]),
            ),
            (
                "beta".to_owned(),
                BTreeMap::from([(
                    "B".to_owned(),
                    crate::passes::visibility_annotation::VisibilityTier::Publica,
                )]),
            ),
        ]),
        two_import_interfaces(),
    )
}

/// Run the two-import consumer analysis and return the flat diagnostic list.
fn two_import_diagnostics(source: &str) -> Vec<crate::SemanticError> {
    analyze_two_import_consumer(source).diagnostics.clone()
}

/// Collect every variant `DefId` the HIR binds through expression paths and
/// pattern variants — the qualified-identity proof surface for the
/// two-import collision fixture.
struct VariantPathCollector {
    def_ids: std::collections::HashSet<radix_hir::DefId>,
}

impl radix_hir::visit::HirVisitor for VariantPathCollector {
    fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
        if let radix_hir::HirExpressionKind::Path(def_id) = &expr.kind {
            self.def_ids.insert(*def_id);
        }
        radix_hir::visit::walk_expr(self, expr);
    }
    fn visit_pattern(&mut self, pattern: &radix_hir::HirPattern) {
        if let radix_hir::HirPattern::Variant(def_id, _) = pattern {
            self.def_ids.insert(*def_id);
        }
        radix_hir::visit::walk_pattern(self, pattern);
    }
}

#[test]
fn two_import_shared_variant_binds_distinct_qualified_identities() {
    // CTO UVN-1 done_when: two imported unions may share the terminal variant
    // name X. `alpha.A.X` and `beta.B.X` bind distinct DefIds through their
    // own imported parent in construction, value, and pattern forms — never
    // through a bare-variant fallback that would collapse the identities.
    let source = r#"importa ex "./alpha" alpha
importa ex "./beta" beta

functio construct_alpha() → alpha.A {
    redde finge alpha.A.X { } ∷ alpha.A
}

functio construct_beta() → beta.B {
    redde finge beta.B.X { } ∷ beta.B
}

functio value_alpha(alpha.A value) → alpha.A {
    redde value
}

functio match_alpha(alpha.A value) → textus {
    discerne value {
        casu alpha.A.X { redde "x" }
    }
}

functio match_beta(beta.B value) → textus {
    discerne value {
        casu beta.B.X { redde "x" }
        ceterum { redde "missing" }
    }
}

incipit {
    sit alpha.A a ← construct_alpha()
    sit beta.B b ← construct_beta()
    sit alpha.A av ← alpha.A.X
    sit beta.B bv ← beta.B.X
    nota value_alpha(av)
    nota match_alpha(a) + match_beta(b)
}"#;
    let mut unit = analyze_two_import_consumer(source);
    assert!(
        unit.is_ok(),
        "two-import qualified forms should analyze green: {:?}",
        unit.diagnostics
    );

    let alpha = unit.interner.intern("alpha");
    let a_name = unit.interner.intern("A");
    let x_name = unit.interner.intern("X");
    let beta = unit.interner.intern("beta");
    let b_name = unit.interner.intern("B");
    let alpha_x = unit
        .resolver
        .namespace_file_type_variant(alpha, a_name, x_name)
        .expect("alpha.A.X must be a known imported variant")
        .0;
    let beta_x = unit
        .resolver
        .namespace_file_type_variant(beta, b_name, x_name)
        .expect("beta.B.X must be a known imported variant")
        .0;
    assert_ne!(
        alpha_x, beta_x,
        "alpha.A.X and beta.B.X must bind distinct DefIds"
    );

    let mut collector = VariantPathCollector {
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
        std::collections::HashSet::from([alpha_x, beta_x]),
        "every bound variant must resolve to exactly its own imported parent identity"
    );
}

#[test]
fn two_import_bare_shared_variant_still_ambiguous() {
    // CTO UVN-1 done_when: the bare terminal name stays an explicit ambiguity
    // — qualification is what disambiguates, and its absence must keep
    // failing closed.
    let source = r#"importa ex "./alpha" alpha
importa ex "./beta" beta

functio construct() → alpha.A {
    redde finge X { } ∷ alpha.A
}"#;
    let diagnostics = two_import_diagnostics(source);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error() && d.issue() == Some("variant_ambiguous")),
        "expected an explicit variant_ambiguous for bare X with two defining unions, got: {diagnostics:?}"
    );
}

#[test]
fn two_import_missing_qualified_variant_fails_closed() {
    // CTO UVN-1 done_when: `alpha.A.Missing` fails closed as an unknown
    // variant — it must never bind through the sibling union's `beta.B.Missing`
    // even though the bare name `Missing` resolves to a single union.
    let source = r#"importa ex "./alpha" alpha
importa ex "./beta" beta

functio construct() → alpha.A {
    redde finge alpha.A.X { } ∷ alpha.A
}

functio match(alpha.A value) → textus {
    discerne value {
        casu alpha.A.Missing { redde "missing" }
    }
}

incipit {
    nota alpha.A.Missing
}"#;
    let diagnostics = two_import_diagnostics(source);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error()
                && matches!(d.issue(), Some("unknown_variant" | "name_not_variant"))),
        "expected a fail-closed unknown-variant error for alpha.A.Missing, got: {diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// Probe P4: local-vs-imported shared variant name (hand-2, be20f61f)
// ---------------------------------------------------------------------------

/// Producer-shaped interface exporting union `Foreign` whose `ElementaMismatch`
/// variant collides with the consumer's local union, plus a `ForeignOnly`
/// variant defined nowhere locally — the mirror of gradus tensor.fab
/// (`TensorError`) vs gradus shape.fab (`FormaError`).
fn colliding_import_interface() -> FileInterface {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Foreign".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            params: Vec::new(),
            name: "Foreign".to_owned(),
            methods: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
            variants: vec![
                InterfaceEnumVariant {
                    name: "ElementaMismatch".to_owned(),
                    fields: Vec::new(),
                },
                InterfaceEnumVariant {
                    name: "ForeignOnly".to_owned(),
                    fields: Vec::new(),
                },
            ],
        }),
    });
    interface
}

fn analyze_collision_consumer(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_with_import_contract(
        source,
        BTreeMap::from([(
            "token".to_owned(),
            BTreeMap::from([(
                "Foreign".to_owned(),
                crate::passes::visibility_annotation::VisibilityTier::Publica,
            )]),
        )]),
        BTreeMap::from([("token".to_owned(), colliding_import_interface())]),
    )
}

/// Run the collision consumer analysis and return the flat diagnostic list.
fn collision_diagnostics(source: &str) -> Vec<crate::SemanticError> {
    analyze_collision_consumer(source).diagnostics.clone()
}

#[test]
fn local_union_shared_variant_name_resolves_bare() {
    // hand-2 be20f61f: gradus tensor.fab declares `TensorError.ElementaMismatch`
    // and imports gradus:shape, whose `FormaError` also declares
    // `ElementaMismatch`. The bare uses in tensor.fab are the LOCAL union's
    // variants — the imported namespace's variant is reachable only qualified
    // (`forma.FormaError.ElementaMismatch`), so it must not turn the local
    // bare name into a false variant_ambiguous. The bare uses bind the local
    // variant identity in pattern and construction forms alike.
    let source = r#"importa ex "./token" token

discretio LocalError {
    ElementaMismatch { textus causa },
    TerminusExcedit { textus causa }
}

functio causa(LocalError e) → textus {
    discerne e {
        casu ElementaMismatch fixum causa { redde causa }
        casu TerminusExcedit fixum causa { redde causa }
    }
}

functio iace() → LocalError ⇥ LocalError {
    iace finge ElementaMismatch { causa = "x" }
}

functio construes() → LocalError {
    redde finge ElementaMismatch { causa = "x" } ∷ LocalError
}

incipit {
    sit LocalError a ← construes()
    nota causa(a)
}"#;
    let mut unit = analyze_collision_consumer(source);
    assert!(
        unit.is_ok(),
        "local union's shared variant name should resolve bare: {:?}",
        unit.diagnostics
    );

    // The bare uses bind the LOCAL variant identity — never the imported one.
    let local_error = unit.interner.intern("LocalError");
    let elementa = unit.interner.intern("ElementaMismatch");
    let token = unit.interner.intern("token");
    let foreign = unit.interner.intern("Foreign");
    let local_x = unit
        .resolver
        .variant_child(
            unit.resolver.lookup(local_error).expect("local union def"),
            elementa,
        )
        .expect("LocalError.ElementaMismatch def");
    let imported_x = unit
        .resolver
        .namespace_file_type_variant(token, foreign, elementa)
        .expect("token.Foreign.ElementaMismatch must be a known imported variant")
        .0;
    assert_ne!(
        local_x, imported_x,
        "local and imported variants must be distinct DefIds"
    );

    let mut collector = VariantPathCollector {
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
    assert!(
        variant_defs.contains(&local_x),
        "bare ElementaMismatch uses must bind the local variant identity, got: {variant_defs:?}"
    );
    assert!(
        !variant_defs.contains(&imported_x),
        "bare ElementaMismatch uses must never bind the imported identity, got: {variant_defs:?}"
    );
}

#[test]
fn local_union_shared_name_does_not_mask_imported_only_bare_name() {
    // The local-wins rule is scoped to names the local union actually defines:
    // a bare name only the imported union defines still resolves through the
    // single-union policy (uvf-u1).
    let source = r#"importa ex "./token" token

discretio LocalError {
    ElementaMismatch { textus causa }
}

incipit {
    sit token.Foreign f ← finge token.ForeignOnly { } ∷ token.Foreign
    sit token.Foreign g ← token.ForeignOnly
    nota f
    nota g
}"#;
    let diagnostics = collision_diagnostics(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| crate::SemanticError::is_error(d))
        .collect();
    assert!(
        errors.is_empty(),
        "imported-only bare name must keep resolving, got: {diagnostics:?}"
    );
}
