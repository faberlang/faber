//! Resolver coverage for relational `schema` declarations (census-types S2-U2;
//! docs/design/relational-views.md §2, stage0/decision-source-column-mapping.md
//! §1.1–§1.2, stage0/fixture-contract.md §3/§6/§7).
//!
//! Proves: a partial schema resolves standalone with zero database dependency;
//! schema names resolve in the type namespace (never as runtime values) per the
//! typus/genus declared-type precedent; the ordered D4 five-field column
//! metadata is built at resolution time; the pinned rejection set
//! (`schema_duplicate_visible`, `schema_duplicate_source`, `schema_unknown_type`,
//! `schema_method`, `schema_inheritance`, `schema_nested_column`) raises with
//! the pinned issue keys; and a schema records exactly its declared columns
//! (partiality — extra source columns stay invisible).

use super::{resolve, Resolver, TypeTable};
use crate::lexer::lex;
use crate::parser::parse;
use crate::semantic::passes::collect;
use crate::semantic::{Primitive, SemanticErrorKind, SymbolKind, Type};
use crate::test_support::args_have_no_legacy_message;
#[cfg(feature = "hir-rust")]
use crate::CompileResult;
use crate::{Compiler, Config};

fn expect_structured_error<'a>(
    errors: &'a [crate::semantic::SemanticError],
    kind: SemanticErrorKind,
    issue: &str,
) -> &'a crate::semantic::SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.kind == kind
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)
        })
        .expect("structured resolve diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated resolve diagnostic should not choose prose at emit site"
    );
    err
}

fn expect_diagnostic<'a>(
    diagnostics: &'a [radix_diagnostics::Diagnostic],
    code: &str,
    label: &str,
) -> &'a radix_diagnostics::Diagnostic {
    let diag = diagnostics
        .iter()
        .find(|d| d.code == Some(code) && d.issue() == Some(label))
        .unwrap_or_else(|| panic!("diagnostic {code}.{label} not found"));
    assert!(
        args_have_no_legacy_message(&diag.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    diag
}

fn compile_and_expect(source: &str, code: &str, label: &str) -> Vec<radix_diagnostics::Diagnostic> {
    let compiler = Compiler::new(Config::default());
    let result = compiler.compile_str("test.fab", source);
    expect_diagnostic(&result.diagnostics, code, label);
    result.diagnostics
}

#[cfg(feature = "hir-rust")]
fn compile_ok(source: &str) -> CompileResult {
    let compiler = Compiler::new(Config::default());
    compiler.compile_str("test.fab", source)
}

/// Parse `source`, run collect + resolve, and return the resolver + type
/// table for direct assertions (drives only the resolve boundary; HIR/typecheck
/// are later units).
fn resolve_program(source: &str) -> (crate::parser::ParseResult, Resolver, TypeTable) {
    let parsed = parse(lex(source));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    {
        let program = parsed.program.as_ref().expect("program");
        let _ = collect::collect(program, &mut resolver, &mut types);
        let result = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);
        assert!(result.is_ok(), "resolve failed: {result:?}");
    }
    (parsed, resolver, types)
}

/// Parse `source`, run collect + resolve, and return the accumulated semantic
/// errors (the caller asserts the pinned rejection).
fn resolve_errors(source: &str) -> Vec<crate::semantic::SemanticError> {
    let parsed = parse(lex(source));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    let _ = collect::collect(program, &mut resolver, &mut types);
    match resolve::resolve(program, &mut resolver, &parsed.interner, &mut types) {
        Ok(()) => Vec::new(),
        Err(errors) => errors,
    }
}

const PERSONA_QUAERENDA: &str = "schema PersonaQuaerenda {
    columna numerus id
    columna textus nomen : plenum
    columna bivalens activa : statim
}";

// =============================================================================
// Standalone resolution + no-DB
// =============================================================================

#[test]
fn schema_resolves_standalone_without_database() {
    // Stage gate 1 (design §2): a partial schema resolves independently of any
    // database connection — the resolve path is purely source-level. The
    // resolve boundary (collect + resolve) accepts the fixture schema with no
    // errors and no catalog/driver dependency.
    //
    // NOTE: end-to-end pipeline acceptance (HIR item lowering) is S2-U3's
    // done_when; S2-U2 proves the resolution boundary.
    let (mut parsed, resolver, _types) = resolve_program(PERSONA_QUAERENDA);
    let schema_sym = parsed.interner.intern("PersonaQuaerenda");
    let schema_def_id = resolver
        .lookup_type(schema_sym)
        .expect("schema in type namespace");
    let schema = resolver.schema_def(schema_def_id).expect("schema metadata");
    assert_eq!(schema.columns.len(), 3);
}

#[test]
fn schema_metadata_carries_ordered_d4_fields() {
    // D4 five-field list (decision-source-column-mapping.md §1.1), canonical
    // column order preserved.
    let (mut parsed, resolver, types) = resolve_program(PERSONA_QUAERENDA);
    let schema_sym = parsed.interner.intern("PersonaQuaerenda");
    let schema_def_id = resolver
        .lookup_type(schema_sym)
        .expect("schema in type namespace");
    let schema = resolver.schema_def(schema_def_id).expect("schema metadata");

    assert_eq!(schema.name, schema_sym);
    assert_eq!(schema.columns.len(), 3);

    // Column 0: identity mapping (no alias row), required, canonical order 0.
    assert_eq!(schema.columns[0].name, parsed.interner.intern("id"));
    assert_eq!(schema.columns[0].source, parsed.interner.intern("id"));
    assert_eq!(schema.columns[0].ty, types.primitive(Primitive::Numerus));
    assert!(schema.columns[0].required);
    assert_eq!(schema.columns[0].order, 0);

    // Column 1: visible rename `nomen` → source `plenum`, required, order 1.
    assert_eq!(schema.columns[1].name, parsed.interner.intern("nomen"));
    assert_eq!(schema.columns[1].source, parsed.interner.intern("plenum"));
    assert_eq!(schema.columns[1].ty, types.primitive(Primitive::Textus));
    assert!(schema.columns[1].required);
    assert_eq!(schema.columns[1].order, 1);

    // Column 2: source-only predicate-column rename `activa` → `statim`.
    assert_eq!(schema.columns[2].name, parsed.interner.intern("activa"));
    assert_eq!(schema.columns[2].source, parsed.interner.intern("statim"));
    assert_eq!(schema.columns[2].ty, types.primitive(Primitive::Bivalens));
    assert!(schema.columns[2].required);
    assert_eq!(schema.columns[2].order, 2);
}

#[test]
fn nullable_columns_derive_required_from_union_type() {
    // `T ∪ nihil` in the type position spells nullability (field 4); the
    // lowered column type is `Type::Option(T)`.
    let (mut parsed, resolver, types) = resolve_program(
        "schema PersonaAmpla {
            columna numerus id
            columna textus ∪ nihil email : electronica
            columna instans ∪ nihil created_at : creata
        }",
    );
    let schema_sym = parsed.interner.intern("PersonaAmpla");
    let schema_def_id = resolver
        .lookup_type(schema_sym)
        .expect("schema in type namespace");
    let schema = resolver.schema_def(schema_def_id).expect("schema metadata");

    assert!(schema.columns[0].required);
    assert_eq!(schema.columns[0].ty, types.primitive(Primitive::Numerus));

    assert!(!schema.columns[1].required);
    assert_eq!(schema.columns[1].source, parsed.interner.intern("electronica"));
    assert!(matches!(types.get(schema.columns[1].ty), Type::Option(_)));

    assert!(!schema.columns[2].required);
    assert_eq!(schema.columns[2].source, parsed.interner.intern("creata"));
    assert!(matches!(types.get(schema.columns[2].ty), Type::Option(_)));
}

#[test]
fn schema_records_exactly_its_declared_columns() {
    // Partiality (design §2.1 / D1 invariant 1): the schema records exactly
    // its declared columns; extra source columns are invisible and harmless.
    let (mut parsed, resolver, _types) = resolve_program(PERSONA_QUAERENDA);
    let schema_sym = parsed.interner.intern("PersonaQuaerenda");
    let schema_def_id = resolver
        .lookup_type(schema_sym)
        .expect("schema in type namespace");
    let schema = resolver.schema_def(schema_def_id).expect("schema metadata");
    assert_eq!(
        schema.columns.len(),
        3,
        "schema must record exactly its declared columns, never the wider source relation"
    );
}

// =============================================================================
// Type-namespace presence
// =============================================================================

#[test]
fn schema_name_resolves_in_type_namespace() {
    // The schema name is a type-namespace member per the typus/genus declared
    // type precedent: usable in type position (here an alias body — the S3
    // `series<S>` / `filtrum<I,O>` type-argument slot) with no runtime value.
    let (mut parsed, resolver, types) =
        resolve_program(&format!("{PERSONA_QUAERENDA}\n\ntypus Referentia = PersonaQuaerenda"));

    let schema_sym = parsed.interner.intern("PersonaQuaerenda");
    let schema_def_id = resolver
        .lookup_type(schema_sym)
        .expect("schema in type namespace");
    assert_eq!(
        resolver
            .get_symbol(schema_def_id)
            .expect("schema symbol")
            .kind,
        SymbolKind::Schema,
        "schema name must resolve as a schema kind, never a runtime value"
    );

    let alias_sym = parsed.interner.intern("Referentia");
    let alias_def_id = resolver.lookup(alias_sym).expect("alias def");
    let symbol = resolver.get_symbol(alias_def_id).expect("alias symbol");
    let ty = symbol.ty.expect("alias must carry a lowered type");
    assert!(
        matches!(types.get(ty), Type::Struct(_)),
        "schema carrier expected in the declared-type slot, got {:?}",
        types.get(ty)
    );
}

// =============================================================================
// Rejections (pinned issue keys)
// =============================================================================

#[test]
fn duplicate_visible_name_raises_schema_duplicate_visible() {
    // DUPLICATE-VISIBLE: the same public name declared twice.
    let result = compile_and_expect(
        "schema S {
            columna numerus id
            columna textus nomen
            columna bivalens nomen
        }",
        "SEM005",
        "schema_duplicate_visible",
    );
    assert_eq!(
        result
            .iter()
            .filter(|d| d.issue() == Some("schema_duplicate_visible"))
            .count(),
        1,
        "exactly one duplicate-visible issue expected"
    );
}

#[test]
fn duplicate_source_name_raises_schema_duplicate_source() {
    // DUP-RESOLVED (fixture-contract §6.2): `nomen` aliases source `plenum`
    // while `plenum` is requested directly — both resolve to one source.
    let result = compile_and_expect(
        "schema S {
            columna textus nomen : plenum
            columna textus plenum
        }",
        "SEM005",
        "schema_duplicate_source",
    );
    assert_eq!(
        result
            .iter()
            .filter(|d| d.issue() == Some("schema_duplicate_source"))
            .count(),
        1,
        "exactly one duplicate-source issue expected"
    );
}

#[test]
fn unknown_column_type_raises_schema_unknown_type() {
    // Column type annotation names a type not in the type namespace.
    let result = compile_and_expect(
        "schema S {
            columna IgnotaRes nomen
        }",
        "SEM002",
        "schema_unknown_type",
    );
    let diag = result
        .iter()
        .find(|d| d.issue() == Some("schema_unknown_type"))
        .expect("schema_unknown_type diagnostic");
    assert_eq!(diag.issue(), Some("schema_unknown_type"));
}

#[test]
fn rejections_raise_semantically_with_structured_issues() {
    // Direct resolve-boundary proof: the pinned issues are raised by the
    // semantic pass itself as structured diagnostics (DUP-RESOLVED from the
    // alias collision; unknown type from a column type outside the namespace).
    let errors = resolve_errors(
        "schema S {
            columna numerus id
            columna textus nomen : plenum
            columna textus plenum
            columna IgnotaRes malum
        }",
    );
    expect_structured_error(&errors, SemanticErrorKind::DuplicateDefinition, "schema_duplicate_source");
    expect_structured_error(&errors, SemanticErrorKind::UndefinedType, "schema_unknown_type");
}

#[test]
fn nested_schema_column_raises_schema_nested_column() {
    // Column whose type is itself a schema (the semantic nested-column class).
    let result = compile_and_expect(
        "schema A {
            columna numerus id
        }

        schema B {
            columna A interior
        }",
        "SEM008",
        "schema_nested_column",
    );
    assert_eq!(
        result
            .iter()
            .filter(|d| d.issue() == Some("schema_nested_column"))
            .count(),
        1,
        "exactly one nested-column issue expected"
    );
}

#[test]
fn method_member_raises_pinned_schema_method_key() {
    // Grammar-level rejection (S2-U1 mechanism) carries the pinned semantic
    // identity through the pipeline: method-like members are rejected before
    // the AST exists, with `schema_method` as the issue fact.
    compile_and_expect(
        "schema S {
            columna numerus id
            functio f() → vacuum {}
        }",
        "PARSE010",
        "schema_method",
    );
}

#[test]
fn inheritance_syntax_raises_pinned_schema_inheritance_key() {
    // `sub`/`implet` inheritance syntax on a schema is rejected at parse time
    // with the pinned `schema_inheritance` issue identity.
    compile_and_expect("schema S sub T { }", "PARSE010", "schema_inheritance");
}

// =============================================================================
// No-regression anchors
// =============================================================================

#[cfg(feature = "hir-rust")]
#[test]
fn bare_series_keyword_anchors_unregressed() {
    // Bare `series` remains the untyped dynamic row map special case
    // (goal success criterion 9); parameterized `series<T>` keeps its pinned
    // `series_params` rejection.
    let result = compile_ok("functio f(series x) → series { redde x }");
    let hard: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| !matches!(d.code, Some(code) if code.starts_with("WARN")))
        .map(|d| d.code.unwrap_or_default().to_string())
        .collect();
    assert!(hard.is_empty(), "bare series must stay unregressed, got {hard:?}");

    compile_and_expect("typus Bad = series<numerus>", "SEM008", "series_params");
}

#[test]
fn schema_kind_is_never_a_value_name() {
    // Schema names resolve in the type namespace and are never runtime values:
    // the symbol kind excludes them from value-name visibility.
    let mut parsed = parse(lex(PERSONA_QUAERENDA));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(program, &mut resolver, &mut types);
    let result = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);
    assert!(result.is_ok(), "schema must resolve: {result:?}");

    let schema_sym = parsed.interner.intern("PersonaQuaerenda");
    let schema_def_id = resolver
        .lookup_type(schema_sym)
        .expect("schema in type namespace");
    let symbol = resolver.get_symbol(schema_def_id).expect("schema symbol");
    assert!(
        !symbol.kind.is_value_name(),
        "a schema name is a type-namespace identity, never a runtime value"
    );
    assert!(symbol.kind.is_type(), "schema names must resolve in type syntax");
}
