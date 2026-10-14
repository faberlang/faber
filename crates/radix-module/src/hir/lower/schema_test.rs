//! Tests for schema declaration lowering (census-types S2-U3).
//!
//! Contract: a `schema` decl lowers to a type-level `HirItemKind::Schema`
//! item carrying the D4 five-field ordered per-column metadata (public name,
//! resolved source identity, Faber type, required/nullability, canonical
//! order), the compiler pipeline accepts schema decls end-to-end without
//! error, and the item survives the HIR artifact serialize→decode round-trip
//! deterministically. Schema decls are never lowered to MIR or any backend
//! leaf.

use super::{LowerRequest, lower};
use crate::hir::{HirItemKind, HirModule};
use crate::semantic::{Primitive, Resolver, Type, TypeTable, passes};

fn prepare_lower_pipeline(source: &str) -> (crate::syntax::Program, Resolver, TypeTable, crate::lexer::Interner) {
    let lex_result = crate::lexer::lex(source);
    let parse_result = crate::parser::parse(lex_result);
    assert!(parse_result.success(), "parse failed: {:?}", parse_result.errors);

    let program = parse_result.program.expect("expected parsed program");
    let mut interner = parse_result.interner;
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();

    crate::builtins::register(&mut resolver, &mut types, &mut interner);
    crate::builtins::register_frame_types_if_needed(&program, &mut resolver, &mut types, &mut interner, None);

    assert!(passes::collect::collect(&program, &mut resolver, &mut types).is_ok());
    if let Err(errors) = passes::resolve::resolve(&program, &mut resolver, &interner, &mut types) {
        panic!("resolve failed: {errors:?}");
    }

    (program, resolver, types, interner)
}

fn lower_schema_source(source: &str) -> (crate::hir::LoweredProgram, crate::lexer::Interner, TypeTable) {
    let (program, mut resolver, mut types, mut interner) = prepare_lower_pipeline(source);
    let (lowered, errors) = lower(LowerRequest {
        program: &program,
        resolver: &mut resolver,
        types: &mut types,
        interner: &mut interner,
        cli_program: None,
        locale_pack: None,
        annotation_contracts: None,
    });
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    (lowered, interner, types)
}

/// The canonical fixture pair: a required+nullable column set covering the
/// identity mapping and the alias rows (fixture-contract §7).
const PERSONA_BREVIS: &str = r"schema PersonaBrevis {
    columna numerus id
    columna textus ∪ nihil nomen : plenum
    columna textus activa : statim
}";

fn schema_item(hir: &HirModule) -> &crate::hir::HirSchema {
    let HirItemKind::Schema(schema) = &hir.items[0].kind else {
        panic!("expected a lowered schema item, got {:?}", hir.items[0].kind);
    };
    schema
}

#[test]
fn lowers_schema_with_ordered_d4_metadata() {
    let (lowered, interner, types) = lower_schema_source(PERSONA_BREVIS);
    let schema = schema_item(&lowered.hir);

    assert_eq!(interner.resolve(schema.name), "PersonaBrevis");
    assert_eq!(schema.columns.len(), 3);

    // Canonical column order is preserved (D4 field 5).
    let names: Vec<&str> = schema
        .columns
        .iter()
        .map(|col| interner.resolve(col.name))
        .collect();
    assert_eq!(names, ["id", "nomen", "activa"]);

    let id = &schema.columns[0];
    assert_eq!(interner.resolve(id.name), "id");
    // Identity mapping: missing alias resolves the source to the public name.
    assert_eq!(interner.resolve(id.source), "id");
    assert_eq!(types.get(id.ty), &Type::Primitive(Primitive::Numerus));
    assert!(!id.nullable);
    assert_eq!(id.order, 0);

    let nomen = &schema.columns[1];
    assert_eq!(interner.resolve(nomen.name), "nomen");
    assert_eq!(interner.resolve(nomen.source), "plenum");
    assert_eq!(types.get(nomen.ty), &Type::Option(types.primitive(Primitive::Textus)));
    // `T ∪ nihil` in the type position derives nullability.
    assert!(nomen.nullable);
    assert_eq!(nomen.order, 1);

    let activa = &schema.columns[2];
    assert_eq!(interner.resolve(activa.name), "activa");
    assert_eq!(interner.resolve(activa.source), "statim");
    assert!(!activa.nullable);
    assert_eq!(activa.order, 2);
}

#[test]
fn lowers_nullable_column_type_to_option() {
    // `T ∪ nihil` lowers to the semantic `Option<T>` shape, and the metadata
    // flag derives from that same type position.
    let (lowered, interner, types) = lower_schema_source(PERSONA_BREVIS);
    let schema = schema_item(&lowered.hir);
    let nomen = &schema.columns[1];
    assert_eq!(interner.resolve(nomen.name), "nomen");
    assert_eq!(types.get(nomen.ty), &Type::Option(types.primitive(Primitive::Textus)));
    assert!(nomen.nullable);
}

#[test]
fn pipeline_accepts_schema_decl_end_to_end() {
    // The compiler pipeline accepts a schema decl through the HIR boundary
    // without error (S2-U3 done_when): lex → parse → collect → resolve →
    // lower all succeed, and the item lands in HIR.
    let (lowered, _interner, _types) = lower_schema_source(PERSONA_BREVIS);
    assert!(matches!(lowered.hir.items[0].kind, HirItemKind::Schema(_)));
}

#[test]
#[cfg(feature = "hir-fhir")]
fn schema_artifact_round_trip_preserves_ordered_metadata() {
    use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
    use handroll::Sha256;

    let session = Session::new(Config::default());
    let unit =
        analyze_source(&session, "schema_test.fab", PERSONA_BREVIS, AnalyzeRequest::default()).expect("analysis");
    let content_hash = {
        let mut hasher = Sha256::new();
        hasher.update(PERSONA_BREVIS.as_bytes());
        hasher.finalize_hex()
    };
    let artifact = crate::hir::serialize::from_analyzed(&unit, &content_hash, "schema_test.fab");
    let bytes = radix_hir_fhir::encode(&artifact).expect("encode");
    let loaded = crate::hir::artifact::load_fhir(&bytes).expect("load");

    let original = schema_item(&unit.hir);
    let decoded = schema_item(&loaded.hir);

    assert_eq!(decoded, original, "schema item must round-trip deterministically");
    assert_eq!(decoded.columns.len(), original.columns.len());
}

#[test]
#[cfg(feature = "hir-faber")]
fn canonical_reemits_schema_decl() {
    // The HIR-backed emitter must preserve schema decls (no silent drop, no
    // crash) so the schema surface still has a re-emit smoke.
    let pack = crate::locale::latin_locale_pack();
    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_resolved_locale_pack(pack.clone())
            .with_dev_locale_root(),
    );
    let result = crate::driver::reemit_faber(&session, &pack, "schema_test.fab", PERSONA_BREVIS);
    assert!(result.success(), "canonical emit failed: {:?}", result.diagnostics);
    let code = result.output.expect("canonical output").code;
    assert!(code.contains("schema PersonaBrevis"));
    assert!(code.contains("columna numerus id"));
    assert!(code.contains("nomen : plenum"));
    assert!(code.contains("activa : statim"));
}
