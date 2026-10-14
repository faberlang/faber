//! Union-variant value reference tests (uvf-u4).
//!
//! Covers imported union variants as first-class *values* in const
//! initializers, function arguments, and field initializers (defect-sprint
//! G1 family / SEM010 surfaces). The const-registry fixture mirrors the rivus
//! `keywords.fab` registry shape: a top-level const table whose rows carry
//! imported `token.TokenKind` variant values (the collapse oracle — the
//! `token.token_kind_*()` constructor calls and fn-built registry become
//! deletable on the consumer side).

use std::collections::BTreeMap;

use crate::test_support;

use crate::Primitive;
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceEnumVariant, InterfaceNominalExport,
    InterfaceStructField, InterfaceTypeSnapshot,
};

/// Install a producer-shaped file interface: a public union exports as an
/// `Enum`-kind nominal carrying its variant metadata, exactly as the CLI
/// export assembly records a `discretio` today (same shape as uvf-u1/u3).
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

/// Run the consumer analysis and return the flat diagnostic list.
fn analyze_consumer_diagnostics(source: &str) -> Vec<crate::SemanticError> {
    analyze_consumer(source).diagnostics.clone()
}

/// Assert no error-level diagnostics (warnings allowed).
fn assert_checks_green(source: &str, label: &str) {
    let diagnostics = analyze_consumer_diagnostics(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected {label} to check green, got: {diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// Probe P1: const initializer (rivus keywords.fab const-registry shape)
// ---------------------------------------------------------------------------

#[test]
fn imported_union_variant_value_in_registry_initializer_checks_green() {
    // uvf-u4 done_when (c): the collapse oracle — a registry table whose rows
    // carry imported `token.TokenKind` variant values (replacing the
    // `token.token_kind_*()` constructor calls in `keywords.fab`).
    //
    // D5.7 retired top-level `fixum`/`varia`; the immutable-registry shape
    // moves into the function that builds it. It cannot become a top-level
    // `generis` static instead: D5.9 requires a compile-time-evaluable
    // initializer, and an imported union variant reference (a `Path` to a
    // def outside this module's static set) is not part of the fold
    // evaluator's evaluable surface (`radix-semantic/src/passes/statica.rs`).
    let source = r#"importa ex "./token" token

genus KeywordSpec {
    varia textus text
    varia token.TokenKind ∪ nihil token_kind
}

incipit {
    fixum lista<KeywordSpec> KEYWORDS ← [
        KeywordSpec { text = "palabra", token_kind = token.TokenKind.Palabra },
        KeywordSpec { text = "numerus", token_kind = finge token.TokenKind.Numerus { code = 3 } ∷ token.TokenKind },
        KeywordSpec { text = "vacua", token_kind = nulla }
    ]
    nota KEYWORDS
}"#;
    assert_checks_green(source, "imported-union variant values in a registry table");
}

#[test]
fn imported_union_variant_single_local_initializer_checks_green() {
    // The simplest surface: a single imported variant value bound to a
    // local binding. Was a top-level const before D5.7 retired top-level
    // `fixum`; not evaluable as a `generis` static for the same reason as
    // the registry-table case above, so it moves into a function body.
    let source = r#"importa ex "./token" token

incipit {
    fixum token.TokenKind PALABRA ← token.TokenKind.Palabra
    nota PALABRA
}"#;
    assert_checks_green(
        source,
        "imported-union variant value in a single local initializer",
    );
}

// ---------------------------------------------------------------------------
// Probe P2: argument position
// ---------------------------------------------------------------------------

#[test]
fn imported_union_variant_value_as_argument_checks_green() {
    // uvf-u4 done_when (b): an imported variant value passes as an argument
    // where the parameter type is the imported union.
    let source = r#"importa ex "./token" token

functio describe(token.TokenKind kind) → textus {
    redde "kind"
}

incipit {
    nota describe(token.TokenKind.Palabra)
    nota describe(finge token.TokenKind.Numerus { code = 5 } ∷ token.TokenKind)
}"#;
    assert_checks_green(source, "imported-union variant values as arguments");
}

// ---------------------------------------------------------------------------
// Probe P3: field initializer (G1-field-init repro)
// ---------------------------------------------------------------------------

#[test]
fn imported_union_variant_value_in_field_initializer_checks_green() {
    // uvf-u4 done_when (b): an imported variant value initializes a genus
    // field whose type is the imported union.
    let source = r#"importa ex "./token" token

genus TokenRow {
    varia token.TokenKind kind
}

incipit {
    fixum TokenRow row ← TokenRow { kind = token.TokenKind.Palabra }
    nota row
}"#;
    assert_checks_green(
        source,
        "imported-union variant values in field initializers",
    );
}

#[test]
fn imported_union_variant_value_in_nullable_field_initializer_checks_green() {
    // The field-init surface through a nullable union annotation
    // (`T ∪ nihil`), the registry-row shape from rivus keywords.fab.
    let source = r#"importa ex "./token" token

genus KeywordSpec {
    varia textus text
    varia token.TokenKind ∪ nihil token_kind
}

incipit {
    fixum KeywordSpec s ← KeywordSpec { text = "x", token_kind = token.TokenKind.Palabra }
    nota s
}"#;
    assert_checks_green(
        source,
        "imported-union variant values in nullable field initializers",
    );
}

// ---------------------------------------------------------------------------
// Fail-closed regressions
// ---------------------------------------------------------------------------

#[test]
fn imported_union_missing_variant_value_still_errors() {
    // A value reference naming a variant the imported union does not define
    // keeps failing closed (never a silent identity).
    let source = r#"importa ex "./token" token

incipit {
    nota token.TokenKind.Perculsus
}"#;
    let diagnostics = analyze_consumer_diagnostics(source);
    assert!(
        diagnostics
            .iter()
            .any(|d| { d.is_error() && matches!(d.issue(), Some("unknown_variant")) }),
        "expected an unknown-variant error for a missing imported variant value, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_value_reference_in_local_initializer_checks_green() {
    // uvf-u4 done_when (b): the same value reference works in an ordinary
    // local initializer (the same SEM010 family as const position).
    let source = r#"importa ex "./token" token

incipit {
    fixum token.TokenKind t ← token.TokenKind.Palabra
    nota t
}"#;
    assert_checks_green(
        source,
        "imported-union variant values in local initializers",
    );
}

#[test]
fn same_module_union_value_references_regression_checks_green() {
    // done_when (e): existing same-module enum/union value behavior is
    // unchanged — the enum shorthand (`Eventum.Sonitus`) still binds.
    test_support::assert_accepts_la(
        r"discretio Eventum {
    Ictus { numerus vis },
    Sonitus
}

incipit {
    fixum Eventum b ← Eventum.Sonitus
    fixum Eventum a ← finge Ictus { vis = 3 } ∷ Eventum
    nota b
}",
    );
}
