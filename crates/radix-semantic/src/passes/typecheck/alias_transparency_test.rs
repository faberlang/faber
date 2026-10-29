//! S0-G2 — alias value transparency proofs.
//!
//! A transparent alias (`typus Vector<magnitudo N> = vector<f32, size N>`) is
//! value-transparent: intrinsic methods, glyph operators, `↦` literal
//! conversions, and builtin↔alias assignment check against the substituted
//! builtin carrier, while genuine width mismatches keep the builtin
//! diagnostics and applied *nominal* receivers stay opaque.

use std::collections::BTreeMap;

use crate::test_support;
use crate::{
    Primitive,
    file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceAliasExport, InterfaceGenericParam,
        InterfaceIndexSnapshot, InterfaceTypeSnapshot,
    },
    passes::visibility_annotation::VisibilityTier,
};

const LOCAL_ALIASES: &str = "typus Vector<magnitudo N> = vector<f32, magnitudo N>
typus Matrix<magnitudo R, magnitudo C> = matrix<f32, [R, C]>
";

// ---------------------------------------------------------------------------
// Positive: methods, glyphs, conversions, assignment, calls
// ---------------------------------------------------------------------------

#[test]
fn intrinsic_methods_check_on_applied_alias_receivers() {
    test_support::assert_accepts(&format!(
        "{LOCAL_ALIASES}\
functio probe(Vector<3> a, Vector<3> b) → f32 {{
    sit Vector<3> swapped ← a.swizzle(\"zyx\")
    redde a.addita(b).productum(a) + swapped.productum(a) + a.transversum(b).productum(a)
}}"
    ));
}

#[test]
fn glyph_operators_check_on_applied_alias_operands() {
    test_support::assert_accepts(&format!(
        "{LOCAL_ALIASES}\
functio probe(Vector<3> a, Vector<2> b) → f32 {{
    sit Matrix<3, 2> outer ← a ⊗ b
    sit Matrix<2, 3> inner ← b ⊗ a
    sit Matrix<3, 3> product ← outer · inner
    redde (product · a).productum(a)
}}"
    ));
}

#[test]
fn literal_conversio_targets_applied_alias() {
    test_support::assert_accepts(&format!(
        "{LOCAL_ALIASES}\
functio probe() → f32 {{
    sit Vector<2> a ← [1.0, 2.0] ↦ Vector<2>
    sit Vector<3> b ← [1.0, 2.0, 3.0] ↦ Vector<3>
    sit Vector<4> c ← [1.0, 2.0, 3.0, 4.0] ↦ Vector<4>
    redde a.productum(a) + b.productum(b) + c.productum(c)
}}"
    ));
}

#[test]
fn builtin_value_assigns_to_alias_annotation() {
    // The gap/alias-assignment-transparency row: a builtin outer product
    // bound to an alias-annotated local.
    test_support::assert_accepts(&format!(
        "{LOCAL_ALIASES}\
functio probe() → f32 {{
    sit vector<f32, magnitudo 4> s ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, magnitudo 4>
    sit vector<f32, magnitudo 3> t ← [1.0, 2.0, 3.0] ↦ vector<f32, magnitudo 3>
    sit Matrix<4, 3> outer ← s ⊗ t
    redde (outer · t).productum(s)
}}"
    ));
}

#[test]
fn generic_call_infers_through_alias_args() {
    test_support::assert_accepts(&format!(
        "{LOCAL_ALIASES}\
functio addita2<magnitudo N>(Vector<N> a, Vector<N> b) → Vector<N> {{
    redde a.addita(b)
}}

functio probe() → f32 {{
    sit Vector<2> a ← [1.0, 2.0] ↦ Vector<2>
    sit Vector<2> b ← [2.0, 4.0] ↦ Vector<2>
    sit Vector<2> s ← addita2(a, b)
    redde s.productum(b)
}}"
    ));
}

#[test]
fn concrete_alias_signature_call_checks_green() {
    // The alias's own param must not leak into the caller's shape-param set
    // (D6): a non-generic function over concrete alias applications checks.
    test_support::assert_accepts(&format!(
        "{LOCAL_ALIASES}\
functio transversum3(Vector<3> a, Vector<3> b) → Vector<3> {{
    redde a.transversum(b)
}}

functio probe() → f32 {{
    sit Vector<3> a ← [1.0, 2.0, 3.0] ↦ Vector<3>
    sit Vector<3> x ← transversum3(a, a)
    redde x.productum(a)
}}"
    ));
}

// ---------------------------------------------------------------------------
// Negative: genuine mismatches keep the builtin diagnostics
// ---------------------------------------------------------------------------

#[test]
fn wrong_width_literal_conversio_keeps_builtin_diagnostic() {
    // Same identity as `[1.0, 2.0] ↦ vector<f32, size 3>`.
    test_support::assert_rejects_with_issue(
        &format!(
            "{LOCAL_ALIASES}\
functio probe() → f32 {{
    sit Vector<3> v ← [1.0, 2.0] ↦ Vector<3>
    redde v.productum(v)
}}"
        ),
        "vector_constructor_width_arity",
    );
}

#[test]
fn builtin_to_alias_wrong_width_assignment_still_rejects() {
    test_support::assert_rejects_with_issue(
        &format!(
            "{LOCAL_ALIASES}\
functio probe() → f32 {{
    sit vector<f32, magnitudo 3> s ← [1.0, 2.0, 3.0] ↦ vector<f32, magnitudo 3>
    sit vector<f32, magnitudo 4> t ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, magnitudo 4>
    sit Matrix<4, 3> bad ← s ⊗ t
    redde bad.productum(bad)
}}"
        ),
        "incompatible_tensor_index",
    );
}

#[test]
fn generic_call_width_mismatch_still_rejects() {
    test_support::assert_rejects_with_issue(
        &format!(
            "{LOCAL_ALIASES}\
functio addita2<magnitudo N>(Vector<N> a, Vector<N> b) → Vector<N> {{
    redde a.addita(b)
}}

functio probe() → f32 {{
    sit Vector<2> a ← [1.0, 2.0] ↦ Vector<2>
    sit Vector<3> b ← [1.0, 2.0, 3.0] ↦ Vector<3>
    sit Vector<2> s ← addita2(a, b)
    redde s.productum(a)
}}"
        ),
        "incompatible_tensor_index",
    );
}

#[test]
fn unknown_method_on_alias_receiver_keeps_rejecting() {
    // The carrier's closed method surface governs alias receivers too.
    test_support::assert_rejects_with_issue(
        &format!(
            "{LOCAL_ALIASES}\
functio probe(Vector<3> a) → f32 {{
    redde a.nusquam_methodus()
}}"
        ),
        "unknown_method",
    );
}

#[test]
fn applied_nominal_receiver_keeps_unknown_method() {
    // Transparency is for aliases, not nominals: a generic nominal class
    // application does not expose the carrier's intrinsic surface.
    test_support::assert_rejects_with_issue(
        "genus Capsula<magnitudo N> {
    varia vector<f32, magnitudo N> lanes
}

functio probe(Capsula<3> c) → f32 {
    redde c.addita(c)
}",
        "unknown_method",
    );
}

// ---------------------------------------------------------------------------
// Imported alias transparency (the runner's gap-row shape)
// ---------------------------------------------------------------------------

/// Producer-shaped interface: a public size-parametric alias exported with
/// its declaration params and parametric body, exactly as the interface
/// route serializes `@ public type Matrix<size R, size C> = …`.
fn alias_provider_interface() -> FileInterface {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Matrix".to_owned(),
        kind: FileExportKind::TypeAlias(InterfaceAliasExport {
            name: "Matrix".to_owned(),
            params: vec![
                InterfaceGenericParam {
                    name: "R".to_owned(),
                    kind: radix_syntax::GenericParamKind::Magnitudo,
                },
                InterfaceGenericParam {
                    name: "C".to_owned(),
                    kind: radix_syntax::GenericParamKind::Magnitudo,
                },
            ],
            body: InterfaceTypeSnapshot::Matrix(
                Box::new(InterfaceTypeSnapshot::SizedNumeric(
                    Primitive::Fractus,
                    radix_types::NumericWidth::F32,
                )),
                InterfaceIndexSnapshot::Tuple(vec![
                    InterfaceIndexSnapshot::Param("R".to_owned()),
                    InterfaceIndexSnapshot::Param("C".to_owned()),
                ]),
            ),
        }),
    });
    interface
}

fn analyze_imported(source: &str) -> crate::test_support::AnalyzedFixture {
    test_support::analyze_source_with_import_contract(
        source,
        BTreeMap::from([(
            "gmt".to_owned(),
            BTreeMap::from([("Matrix".to_owned(), VisibilityTier::Publica)]),
        )]),
        BTreeMap::from([("gmt".to_owned(), alias_provider_interface())]),
    )
}

#[test]
fn imported_alias_application_is_value_transparent() {
    let fixture = analyze_imported(
        r#"importa ex "./gmt" gmt

functio probe() → f32 {
    sit vector<f32, magnitudo 4> s ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, magnitudo 4>
    sit vector<f32, magnitudo 3> t ← [1.0, 2.0, 3.0] ↦ vector<f32, magnitudo 3>
    sit gmt.Matrix<4, 3> outer ← s ⊗ t
    redde (outer · t).productum(s)
}"#,
    );
    assert!(
        fixture.result.success(),
        "imported alias assignment should be transparent: {:?}",
        fixture.result.diagnostics
    );
}

#[test]
fn imported_alias_wrong_width_still_rejects() {
    let fixture = analyze_imported(
        r#"importa ex "./gmt" gmt

functio probe() → f32 {
    sit vector<f32, magnitudo 3> s ← [1.0, 2.0, 3.0] ↦ vector<f32, magnitudo 3>
    sit vector<f32, magnitudo 4> t ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, magnitudo 4>
    sit gmt.Matrix<4, 3> bad ← s ⊗ t
    redde (bad · t).productum(t)
}"#,
    );
    assert!(
        !fixture.result.success(),
        "wrong-width imported alias assignment must reject"
    );
    assert!(
        fixture
            .result
            .diagnostics
            .iter()
            .any(|err| err.is_error()
                && test_support::issue(err) == Some("incompatible_tensor_index")),
        "expected incompatible_tensor_index, got: {:?}",
        fixture.result.diagnostics
    );
}
