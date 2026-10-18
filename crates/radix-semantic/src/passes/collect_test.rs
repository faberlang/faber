//! Coverage for the collect pass.
//!
//! These tests prove that the first AST pass registers top-level declarations
//! in the resolver and rejects duplicate definitions.

use super::{Resolver, SemanticErrorKind, SymbolKind, TypeTable, collect};
use crate::SemanticError;
use crate::test_support::args_have_no_legacy_message;
use radix_lexer::lex;
use radix_lexer::{Interner, Span, Symbol};
use radix_parser::parse;
use radix_syntax::{
    AstMutability, BlockStmt, EnumDecl, EnumMember, FuncDecl, ImportDecl, ImportKind, Program,
    Stmt, StmtKind, TypeExpr, TypeExprKind, UnionDecl, VarDecl, Variant, Visibility,
};

fn ident(interner: &mut Interner, name: &str) -> radix_syntax::Ident {
    radix_syntax::Ident {
        name: interner.intern(name),
        span: Span::default(),
    }
}

fn ident_sym(sym: Symbol) -> radix_syntax::Ident {
    radix_syntax::Ident {
        name: sym,
        span: Span::default(),
    }
}

fn stmt(kind: StmtKind) -> Stmt {
    Stmt {
        id: 0,
        kind,
        span: Span::default(),
        annotations: Vec::new(),
        leading_trivia: Vec::new(),
    }
}

fn program(statements: Vec<Stmt>) -> Program {
    Program {
        statements,
        end_trivia: Vec::new(),
        span: Span::default(),
        region: None,
    }
}

fn expect_structured_error<'a>(
    errors: &'a [SemanticError],
    kind: SemanticErrorKind,
    expected_args: &[(&str, &str)],
) -> &'a SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.kind == kind
                && expected_args.iter().all(|(name, value)| {
                    err.args
                        .iter()
                        .any(|arg| arg.name == *name && arg.value == *value)
                })
        })
        .expect("structured semantic diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    err
}

#[test]
fn reports_duplicate_definitions() {
    let mut interner = Interner::new();
    let sym = interner.intern("x");
    let var = VarDecl {
        mutability: AstMutability::Immutable,
        await_form: None,
        ty: None,
        binding: radix_syntax::BindingPattern::Ident(ident_sym(sym)),
        init: None,
        compile_time: false,
    };
    let var2 = VarDecl {
        mutability: AstMutability::Immutable,
        await_form: None,
        ty: None,
        binding: radix_syntax::BindingPattern::Ident(ident_sym(sym)),
        init: None,
        compile_time: false,
    };
    let program = program(vec![stmt(StmtKind::Var(var)), stmt(StmtKind::Var(var2))]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::DefinitionDuplicate,
        &[("issue", "duplicate_definition")],
    );
}

#[test]
fn inferred_import_name_collision_requests_explicit_alias() {
    let mut interner = Interner::new();
    let sym = interner.intern("chorda");
    let var = VarDecl {
        mutability: AstMutability::Immutable,
        await_form: None,
        ty: None,
        binding: radix_syntax::BindingPattern::Ident(ident_sym(sym)),
        init: None,
        compile_time: false,
    };
    let import = ImportDecl {
        path: interner.intern("norma:chorda"),
        visibility: Visibility::Privata,
        kind: ImportKind::Named {
            name: ident_sym(sym),
            alias: None,
            inferred: true,
        },
        target: None,
        span: Span::default(),
    };
    let program = program(vec![
        stmt(StmtKind::Var(var)),
        stmt(StmtKind::Import(import)),
    ]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::DefinitionDuplicate,
        &[("issue", "inferred_import_name_collision")],
    );
}

fn collect_source(source: &str) -> Result<(), Vec<SemanticError>> {
    let parsed = parse(lex(source));
    assert!(parsed.success(), "{:?}", parsed.errors);
    let program = parsed.program.expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(&program, &mut resolver, &mut types)
}

#[test]
fn two_default_import_names_collide() {
    // D7.7: two imports that would default to the same name are an error.
    let errors = collect_source("importa ex \"./a/geo\"\nimporta ex \"./b/geo\"\n")
        .expect_err("default-name collision");
    expect_structured_error(
        &errors,
        SemanticErrorKind::DefinitionDuplicate,
        &[("issue", "default_import_name_collision")],
    );
}

#[test]
fn default_import_name_colliding_with_explicit_binding_is_an_error() {
    let errors = collect_source("importa ex \"./a/geo\" geo\nimporta ex \"./b/geo\"\n")
        .expect_err("default-name collision");
    expect_structured_error(
        &errors,
        SemanticErrorKind::DefinitionDuplicate,
        &[("issue", "default_import_name_collision")],
    );
}

#[test]
fn aliasing_one_default_import_resolves_the_collision() {
    collect_source("importa ex \"./a/geo\"\nimporta ex \"./b/geo\" geo_b\n")
        .expect("aliased imports bind distinct names");
}

#[test]
fn top_level_var_requires_identifier_binding() {
    let program = program(vec![stmt(StmtKind::Var(VarDecl {
        mutability: AstMutability::Immutable,
        await_form: None,
        ty: None,
        binding: radix_syntax::BindingPattern::Wildcard(Span::default()),
        init: None,
        compile_time: false,
    }))]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    expect_structured_error(
        &errors,
        SemanticErrorKind::LoweringError,
        &[("issue", "top_level_identifier_binding")],
    );
}

#[test]
fn collects_enum_variants() {
    let mut interner = Interner::new();
    let enum_name = ident(&mut interner, "Color");
    let red = EnumMember {
        leading_trivia: Vec::new(),
        name: ident(&mut interner, "Red"),
        value: None,
        span: Span::default(),
    };
    let green = EnumMember {
        leading_trivia: Vec::new(),
        name: ident(&mut interner, "Green"),
        value: None,
        span: Span::default(),
    };
    let program = program(vec![stmt(StmtKind::Enum(EnumDecl {
        name: enum_name,
        members: vec![red, green],
    }))]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(result.is_ok());

    // Variants register under the parent-qualified namespace, not the bare
    // name-keyed scope table (variant identity is scoped by the parent).
    assert_eq!(resolver.lookup(interner.intern("Red")), None);
    let enum_def = resolver.lookup(interner.intern("Color")).expect("enum def");
    let red_def = resolver
        .variant_child(enum_def, interner.intern("Red"))
        .expect("red def");
    let symbol = resolver.get_symbol(red_def).expect("red symbol");
    assert_eq!(symbol.kind, SymbolKind::Variant);
    let green_def = resolver
        .variant_child(enum_def, interner.intern("Green"))
        .expect("green def");
    assert_ne!(red_def, green_def, "Red and Green are distinct variants");
}

#[test]
fn collects_union_variants() {
    let mut interner = Interner::new();
    let union_name = ident(&mut interner, "Resultatum");
    let ok = Variant {
        leading_trivia: Vec::new(),
        name: ident(&mut interner, "Bene"),
        fields: Vec::new(),
        span: Span::default(),
    };
    let error = Variant {
        leading_trivia: Vec::new(),
        name: ident(&mut interner, "Error"),
        fields: Vec::new(),
        span: Span::default(),
    };
    let program = program(vec![stmt(StmtKind::Union(UnionDecl {
        name: union_name,
        type_params: Vec::new(),
        shared_fields: Vec::new(),
        variants: vec![ok, error],
    }))]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(result.is_ok());

    // Variants register under the parent-qualified namespace, not the bare
    // name-keyed scope table (variant identity is scoped by the parent).
    assert_eq!(resolver.lookup(interner.intern("Bene")), None);
    let enum_def = resolver
        .lookup(interner.intern("Resultatum"))
        .expect("union enum def");
    let variant_def = resolver
        .variant_child(enum_def, interner.intern("Bene"))
        .expect("union variant def");
    let symbol = resolver
        .get_symbol(variant_def)
        .expect("union variant symbol");
    assert_eq!(symbol.kind, SymbolKind::Variant);
    let error_def = resolver
        .variant_child(enum_def, interner.intern("Error"))
        .expect("union variant def");
    assert_ne!(
        variant_def, error_def,
        "Bene and Error are distinct variants"
    );
}

#[test]
fn cross_union_variant_names_do_not_collide() {
    let mut interner = Interner::new();
    let union_a = UnionDecl {
        name: ident(&mut interner, "A"),
        type_params: Vec::new(),
        shared_fields: Vec::new(),
        variants: vec![Variant {
            leading_trivia: Vec::new(),
            name: ident(&mut interner, "X"),
            fields: Vec::new(),
            span: Span::default(),
        }],
    };
    let union_b = UnionDecl {
        name: ident(&mut interner, "B"),
        type_params: Vec::new(),
        shared_fields: Vec::new(),
        variants: vec![Variant {
            leading_trivia: Vec::new(),
            name: ident(&mut interner, "X"),
            fields: Vec::new(),
            span: Span::default(),
        }],
    };
    let program = program(vec![
        stmt(StmtKind::Union(union_a)),
        stmt(StmtKind::Union(union_b)),
    ]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(
        result.is_ok(),
        "cross-union same-name variants must declare cleanly"
    );

    let a_def = resolver.lookup(interner.intern("A")).expect("union A def");
    let b_def = resolver.lookup(interner.intern("B")).expect("union B def");
    let a_x = resolver
        .variant_child(a_def, interner.intern("X"))
        .expect("A.X def");
    let b_x = resolver
        .variant_child(b_def, interner.intern("X"))
        .expect("B.X def");
    assert_ne!(a_x, b_x, "A.X and B.X must be distinct DefIds");
    assert_eq!(
        resolver.variant_child(a_def, interner.intern("X")),
        Some(a_x)
    );
    assert_eq!(
        resolver.variant_child(b_def, interner.intern("X")),
        Some(b_x)
    );
    // Neither bare name enters the bare scope table.
    assert_eq!(resolver.lookup(interner.intern("X")), None);
    // Bare lookup through the per-union namespace reports the ambiguity.
    assert!(matches!(
        resolver.lookup_variant_bare(interner.intern("X")),
        crate::BareVariantLookup::Ambiguous { defining_unions: 2 }
    ));
}

#[test]
fn same_union_duplicate_variant_still_fails() {
    let mut interner = Interner::new();
    let union = UnionDecl {
        name: ident(&mut interner, "A"),
        type_params: Vec::new(),
        shared_fields: Vec::new(),
        variants: vec![
            Variant {
                leading_trivia: Vec::new(),
                name: ident(&mut interner, "X"),
                fields: Vec::new(),
                span: Span::default(),
            },
            Variant {
                leading_trivia: Vec::new(),
                name: ident(&mut interner, "X"),
                fields: Vec::new(),
                span: Span::default(),
            },
        ],
    };
    let program = program(vec![stmt(StmtKind::Union(union))]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(result.is_err());
    expect_structured_error(
        &result.unwrap_err(),
        SemanticErrorKind::DefinitionDuplicate,
        &[("issue", "duplicate_definition")],
    );
}

#[test]
fn collects_function_definitions() {
    let mut interner = Interner::new();
    let name = ident(&mut interner, "greet");
    let func = FuncDecl {
        name,
        type_params: Vec::new(),
        params: Vec::new(),
        modifiers: Vec::new(),
        posture: radix_syntax::CallablePosture::SyncFinite,
        ret: Some(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(ident(&mut interner, "vacuum"), Vec::new()),
            span: Span::default(),
            form: radix_syntax::TypeFormLevel::LONG,
            iuncta_label: None,
        }),
        err: None,
        body: Some(BlockStmt {
            statements: Vec::new(),
            end_trivia: Vec::new(),
            span: Span::default(),
        }),
        annotations: Vec::new(),
        leading_trivia: Vec::new(),
    };
    let program = program(vec![stmt(StmtKind::Func(func))]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(result.is_ok());

    let def_id = resolver
        .lookup(interner.intern("greet"))
        .expect("function def");
    let symbol = resolver.get_symbol(def_id).expect("function symbol");
    assert_eq!(symbol.kind, SymbolKind::Function);
}

#[test]
fn allows_duplicate_import_module_bindings() {
    let mut interner = Interner::new();
    let path = interner.intern("helpers");
    let import1 = ImportDecl {
        path,
        visibility: Visibility::Privata,
        kind: ImportKind::Named {
            name: ident(&mut interner, "item"),
            alias: None,
            inferred: false,
        },
        target: None,
        span: Span::default(),
    };
    let import2 = ImportDecl {
        path,
        visibility: Visibility::Privata,
        kind: ImportKind::Named {
            name: ident(&mut interner, "item"),
            alias: None,
            inferred: false,
        },
        target: None,
        span: Span::default(),
    };
    let program = program(vec![
        stmt(StmtKind::Import(import1)),
        stmt(StmtKind::Import(import2)),
    ]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(result.is_ok());
}

#[test]
fn import_alias_binds_visible_name() {
    let mut interner = Interner::new();
    let path = interner.intern("helpers");
    let item_name = ident(&mut interner, "originale");
    let alias = ident(&mut interner, "auxilium");
    let import = ImportDecl {
        path,
        visibility: Visibility::Privata,
        kind: ImportKind::Named {
            name: item_name,
            alias: Some(alias),
            inferred: false,
        },
        target: None,
        span: Span::default(),
    };
    let program = program(vec![stmt(StmtKind::Import(import))]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(result.is_ok());

    assert!(resolver.lookup(interner.intern("originale")).is_none());
    let alias_def = resolver
        .lookup(interner.intern("auxilium"))
        .expect("alias def");
    let symbol = resolver.get_symbol(alias_def).expect("alias symbol");
    assert_eq!(symbol.kind, SymbolKind::Module);
}

#[test]
fn empty_program_collects_without_errors() {
    let program = program(vec![]);

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);

    assert!(
        result.is_ok(),
        "empty program should collect without errors"
    );
}

/// uvn-u3 oracle: the five Rivus pilot names (`InvalidAnnotation`,
/// `InvalidAssignmentTarget`, `Warning`, `Lex`, `Parse`) declare cleanly
/// across distinct unions in one module WITHOUT renames, mirroring the union
/// set of the rivus `src/diagnostics/diagnostic.fab` module. The
/// migrate-diagnostics pilot had to rename them to
/// `SemanticInvalidAnnotation`, `SemanticInvalidAssignmentTarget`,
/// `SemanticWarning`, `LexPhase`, `ParsePhase` because union variant names
/// were module-flat; per-union variant identity (uvn-u1) makes the bare names
/// legal again.
#[test]
fn rivus_pilot_names_declare_cleanly_across_distinct_unions() {
    let source = r"
discretio Severity {
    Error,
    Warning,
    Info
}
discretio DiagnosticPhase {
    Io,
    Lex,
    Parse,
    Resolve,
    Lower,
    Typecheck,
    Analysis,
    Mir,
    Codegen,
    Tool
}
discretio ParseErrorKind {
    LexError,
    InvalidAnnotation,
    InvalidAssignmentTarget
}
discretio SemanticErrorKind {
    UndefinedVariable,
    DuplicateDefinition,
    InvalidAnnotation,
    InvalidAssignmentTarget,
    Warning
}
discretio PhaseErrorKind {
    Lex,
    Parse,
    Semantic
}
";
    let mut parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(program, &mut resolver, &mut types)
        .expect("the five pilot names must declare cleanly across distinct unions");

    // Each colliding name is registered under BOTH defining unions with a
    // distinct DefId, and bare lookup through the per-union namespace reports
    // the fail-closed ambiguity rather than a silent pick.
    for (name, first_union, second_union) in [
        ("InvalidAnnotation", "ParseErrorKind", "SemanticErrorKind"),
        (
            "InvalidAssignmentTarget",
            "ParseErrorKind",
            "SemanticErrorKind",
        ),
        ("Warning", "Severity", "SemanticErrorKind"),
        ("Lex", "DiagnosticPhase", "PhaseErrorKind"),
        ("Parse", "DiagnosticPhase", "PhaseErrorKind"),
    ] {
        let first_def = resolver
            .lookup(parsed.interner.intern(first_union))
            .expect("first defining union");
        let second_def = resolver
            .lookup(parsed.interner.intern(second_union))
            .expect("second defining union");
        let first_variant = resolver
            .variant_child(first_def, parsed.interner.intern(name))
            .expect("first union variant");
        let second_variant = resolver
            .variant_child(second_def, parsed.interner.intern(name))
            .expect("second union variant");
        assert_ne!(
            first_variant, second_variant,
            "{name} must be a distinct DefId under {first_union} and {second_union}"
        );
        assert!(matches!(
            resolver.lookup_variant_bare(parsed.interner.intern(name)),
            crate::BareVariantLookup::Ambiguous { defining_unions: 2 }
        ));
    }
}

// =============================================================================
// EOT-1 — import collection baseline and selective red driver
// =============================================================================

#[test]
fn eot_namespace_import_collection_baseline_remains_green() {
    let parsed = parse(lex(r#"importa ex "norma:consolum" consolum"#));
    assert!(
        parsed.errors.is_empty(),
        "legacy namespace import parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("legacy namespace import program");

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(
        result.is_ok(),
        "legacy namespace import must still collect: {result:?}"
    );
}

#[test]
fn eot_selective_import_collection_red_driver() {
    let parsed = parse(lex(
        r#"importa ex "norma:consolum" fixum dic ut output, funde ut output_bytes,"#,
    ));
    assert!(
        parsed.errors.is_empty(),
        "EOT-2M red driver: selective members must reach collection as local bindings; current parser errors={:?}; expected pre-EOT issue is `expected_expression`",
        parsed.errors
    );
    let program = parsed.program.expect("selective import program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let result = collect::collect(&program, &mut resolver, &mut types);
    assert!(
        result.is_ok(),
        "selective bindings must collect independently: {result:?}"
    );
}

// =============================================================================
// EOT-2M — selective binding collection diagnostics
// =============================================================================

#[test]
fn selective_import_duplicate_alias_fails_with_named_issue() {
    let parsed = parse(lex(
        r#"importa ex "norma:consolum" fixum dic ut output, funde ut output,"#,
    ));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("selective import program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let errors = collect::collect(&program, &mut resolver, &mut types)
        .expect_err("duplicate local alias within one list must fail collection");
    assert!(
        errors
            .iter()
            .any(|err| crate::test_support::issue(err) == Some("selective_import_duplicate_alias")),
        "expected selective_import_duplicate_alias, got: {errors:?}"
    );
}

#[test]
fn selective_import_local_collision_reports_ordinary_duplicate() {
    let parsed = parse(lex(r#"functio output() → vacuum { tacet }
importa ex "norma:consolum" fixum dic ut output,"#));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let errors = collect::collect(&program, &mut resolver, &mut types)
        .expect_err("collision with an existing declaration must fail collection");
    assert!(
        errors
            .iter()
            .any(|err| crate::test_support::issue(err) == Some("duplicate_definition")),
        "expected ordinary duplicate_definition collision, got: {errors:?}"
    );
}
