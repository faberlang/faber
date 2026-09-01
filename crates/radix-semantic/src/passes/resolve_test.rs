//! Coverage for the resolve pass.
//!
//! These tests prove that the second AST pass resolves names and type aliases
//! in the typed resolver and digests file-interface type snapshots.

use super::{Resolver, SemanticErrorKind, TypeTable, resolve};
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceLibraryIdentity,
    InterfaceMemberKind, InterfaceMethodExport, InterfaceNominalKind, InterfaceNominalType,
    InterfaceStructExport, InterfaceStructField, InterfaceTypeSnapshot,
};
use crate::passes::collect;
use crate::test_support;
use crate::test_support::args_have_no_legacy_message;
use crate::{Primitive, SemanticParamMode, Type};
use radix_lexer::lex;
use radix_lexer::{Interner, Span};
use radix_parser::parse;
use radix_syntax::{
    BlockStmt, Expr, ExprKind, FuncDecl, GenericArgExpr, IfBody, Param, Program, Stmt, StmtKind,
    TypeExpr, TypeExprKind,
};

fn ident(interner: &mut Interner, name: &str) -> radix_syntax::Ident {
    radix_syntax::Ident {
        name: interner.intern(name),
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
    }
}

fn expect_structured_error<'a>(
    errors: &'a [crate::SemanticError],
    kind: SemanticErrorKind,
    issue: &str,
) -> &'a crate::SemanticError {
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
    diagnostics: &'a [crate::SemanticError],
    code: &str,
    label: &str,
) -> &'a crate::SemanticError {
    let diag = diagnostics
        .iter()
        .find(|d| crate::semantic_spec(d.kind).code == code && d.issue() == Some(label))
        .unwrap_or_else(|| panic!("diagnostic {code}.{label} not found"));
    assert!(
        args_have_no_legacy_message(&diag.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    diag
}

fn compile_and_expect(source: &str, code: &str, label: &str) -> Vec<crate::SemanticError> {
    let result = crate::test_support::analyze_source(source);
    if code.starts_with("PARSE") {
        assert!(
            result.diagnostics.iter().any(|d| d.issue() == Some(label)),
            "parse diagnostic {code}.{label} not found: {:?}",
            result.diagnostics
        );
        return result.diagnostics.clone();
    }
    expect_diagnostic(&result.diagnostics, code, label);
    result.diagnostics.clone()
}

fn compile_ok(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_ok(source)
}

fn named_type(name: radix_syntax::Ident) -> TypeExpr {
    TypeExpr {
        nullable: false,
        mode: None,
        kind: TypeExprKind::Named(name, Vec::new()),
        span: Span::default(),
        form: radix_syntax::TypeFormLevel::LONG,
        iuncta_label: None,
    }
}

fn named_type_with_params(name: radix_syntax::Ident, params: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr {
        nullable: false,
        mode: None,
        kind: TypeExprKind::Named(name, params.into_iter().map(GenericArgExpr::Type).collect()),
        span: Span::default(),
        form: radix_syntax::TypeFormLevel::LONG,
        iuncta_label: None,
    }
}

fn func_with_param_type(interner: &mut Interner, ty: TypeExpr) -> Program {
    let param = Param {
        sponte: false,
        mode: radix_syntax::AstParamMode::Owned,
        is_rest: false,
        ty,
        name: ident(interner, "value"),
        alias: None,
        default: None,
        span: Span::default(),
    };
    let func = FuncDecl {
        name: ident(interner, "f"),
        type_params: Vec::new(),
        params: vec![param],
        modifiers: Vec::new(),
        posture: radix_syntax::CallablePosture::SyncFinite,
        ret: None,
        err: None,
        body: Some(BlockStmt {
            statements: Vec::new(),
            end_trivia: Vec::new(),
            span: Span::default(),
        }),
        annotations: Vec::new(),
        leading_trivia: Vec::new(),
    };
    program(vec![stmt(StmtKind::Func(func))])
}

#[test]
fn reports_undefined_variable_in_expression() {
    compile_and_expect(
        "incipit { fixum _ x ← missing }",
        "SEM001",
        "unknown_identifier",
    );
}

#[test]
fn scribe_family_callee_hints_bare_output_form() {
    compile_and_expect("incipit { sit x ← nota(1) }", "SEM001", "scribe_callee");
}

#[test]
fn unknown_non_scribe_callee_stays_unknown_identifier() {
    compile_and_expect(
        "incipit { sit x ← missing(1) }",
        "SEM001",
        "unknown_identifier",
    );
}

#[test]
fn semantic_undefined_variable_suggests_visible_name() {
    let parsed = parse(lex(r#"
incipit {
  fixum _ greeting ← "salve"
  nota greting
}
"#));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(program, &mut resolver, &mut types).expect("collect");

    let errors = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types)
        .expect_err("resolve should fail");
    let err = expect_structured_error(
        &errors,
        SemanticErrorKind::VariableUndefined,
        "unknown_identifier",
    );

    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "spelling" && arg.value == "greting")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "suggestion" && arg.value == "greeting")
    );
}

#[test]
fn semantic_undefined_variable_does_not_suggest_distant_visible_name() {
    let parsed = parse(lex(r#"
incipit {
  fixum _ greeting ← "salve"
  nota zz
}
"#));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(program, &mut resolver, &mut types).expect("collect");

    let errors = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types)
        .expect_err("resolve should fail");
    let err = expect_structured_error(
        &errors,
        SemanticErrorKind::VariableUndefined,
        "unknown_identifier",
    );

    assert!(
        err.args.iter().all(|arg| arg.name != "suggestion"),
        "distant name should not produce suggestion args: {err:?}"
    );
}

#[test]
fn semantic_undefined_variables_reuse_suggestion_cache() {
    let parsed = parse(lex(r#"
incipit {
  fixum _ greeting ← "salve"
  fixum _ farewell ← "vale"
  nota greting
  nota farewel
}
"#));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(program, &mut resolver, &mut types).expect("collect");

    let errors = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types)
        .expect_err("resolve should fail");
    let mut suggestions: Vec<(String, String)> = errors
        .iter()
        .filter(|err| err.kind == SemanticErrorKind::VariableUndefined)
        .map(|err| {
            let spelling = err
                .args
                .iter()
                .find(|arg| arg.name == "spelling")
                .expect("undefined-variable error carries spelling arg")
                .value
                .clone();
            let suggestion = err
                .args
                .iter()
                .find(|arg| arg.name == "suggestion")
                .expect("undefined-variable error carries suggestion arg")
                .value
                .clone();
            (spelling, suggestion)
        })
        .collect();
    suggestions.sort_unstable();
    assert_eq!(
        suggestions,
        vec![
            ("farewel".to_owned(), "farewell".to_owned()),
            ("greting".to_owned(), "greeting".to_owned()),
        ]
    );
}

#[test]
fn semantic_undefined_variable_suggests_unicode_fold_parity() {
    let parsed = parse(lex(r"
incipit {
  fixum _ rēs ← 1
  nota res
}
"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(program, &mut resolver, &mut types).expect("collect");

    let errors = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types)
        .expect_err("resolve should fail");
    let err = expect_structured_error(
        &errors,
        SemanticErrorKind::VariableUndefined,
        "unknown_identifier",
    );

    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "spelling" && arg.value == "res")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "suggestion" && arg.value == "rēs")
    );
}

#[test]
fn reports_duplicate_local_definition_with_structured_event() {
    let parsed = parse(lex(r"
incipit {
  fixum numerus value ← 1
  fixum numerus value ← 2
}
"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(program, &mut resolver, &mut types).expect("collect");

    let errors = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types)
        .expect_err("resolve should fail");

    expect_structured_error(
        &errors,
        SemanticErrorKind::DefinitionDuplicate,
        "duplicate_definition",
    );
}

#[test]
fn reports_return_outside_function() {
    let diagnostics = compile_and_expect("redde", "SEM032", "return_outside_function");
    let err = expect_diagnostic(&diagnostics, "SEM032", "return_outside_function");
    assert!(err.is_error());
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "keyword" && arg.value == "redde")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "context" && arg.value == "function")
    );
}

#[test]
fn resolves_type_alias_to_builtin() {
    let mut parsed = parse(lex("typus Numeri = numerus"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(program, &mut resolver, &mut types);
    let result = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);
    assert!(result.is_ok());

    let alias_sym = parsed.interner.intern("Numeri");
    let def_id = resolver.lookup(alias_sym).expect("alias def");
    let symbol = resolver.get_symbol(def_id).expect("alias symbol");
    assert_eq!(symbol.ty, Some(types.primitive(Primitive::Numerus)));
}

#[test]
fn resolves_fn_type_param_modes_from_type_position() {
    let mut parsed = parse(lex(
        "typus Handler = (de textus, in lista<numerus>, own lista<numerus>, copy lista<numerus>) → vacuum",
    ));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    resolve::resolve(program, &mut resolver, &parsed.interner, &mut types).expect("resolve");

    let alias_sym = parsed.interner.intern("Handler");
    let def_id = resolver.lookup(alias_sym).expect("alias def");
    let symbol = resolver.get_symbol(def_id).expect("alias symbol");
    let alias_ty = symbol.ty.expect("alias must carry a lowered type");
    let Type::Func(sig) = types.get(alias_ty) else {
        panic!("expected fn type alias, got {:?}", types.get(alias_ty));
    };
    assert_eq!(
        sig.params
            .iter()
            .map(|param| param.mode)
            .collect::<Vec<_>>(),
        vec![
            SemanticParamMode::Ref,
            SemanticParamMode::MutRef,
            SemanticParamMode::Owned,
            SemanticParamMode::Owned,
        ]
    );
}

#[test]
fn resolves_generic_type_alias_own_type_params() {
    // tela-d0: a generic alias must bind its own type parameters while the
    // body is lowered. Before the fix, `resolve_alias_types` lowered the body
    // after `resolve_stmt` had exited the alias scope, so the body's `T`
    // resolved to nothing (SEM008.unknown_type_name).
    let mut parsed = parse(lex("typus X<T> = T"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(program, &mut resolver, &mut types);
    let result = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);
    assert!(result.is_ok(), "generic alias must resolve: {result:?}");

    let alias_sym = parsed.interner.intern("X");
    let def_id = resolver.lookup(alias_sym).expect("alias def");
    let symbol = resolver.get_symbol(def_id).expect("alias symbol");
    let ty = symbol.ty.expect("alias must carry a lowered type");
    // The alias body is the own type parameter: the resolve pass interns the
    // body directly (`Type::Param(T)`), and a reference site wraps it as
    // `Type::Alias(def, body)`. Proving the body is the own `T` proves the
    // param was in scope during lowering.
    assert!(
        matches!(types.get(ty), Type::Param(_)),
        "expected body Type::Param(T), got {:?}",
        types.get(ty)
    );
}

#[test]
fn generic_type_alias_usage_substitutes_type_params() {
    // tela-d0 usage fixture: the alias body `T` binds at declaration, and a
    // use `X<numerus>` expands to `numerus` so the function body typechecks.
    let result =
        compile_ok("typus X<T> = T\n\nfunctio f(numerus x) → X<numerus> {\n    redde x\n}");
    let hard: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| crate::semantic_spec(d.kind).code.to_string())
        .collect();
    assert!(
        hard.is_empty(),
        "generic alias usage must typecheck, got {hard:?}"
    );
}

#[test]
fn generic_type_alias_campaign_shape_resolves() {
    // tela-d0 campaign sketch shape: `type Nuntius<Message> = (Eventum) → Message`
    // checks green at declaration (the unit's (c) oracle). `Eventum` is a
    // declared class; the function-type body uses the alias's own `Message`.
    let result = compile_ok("genus Eventum {}\n\ntypus Nuntius<Message> = (Eventum) → Message");
    let hard: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| crate::semantic_spec(d.kind).code.to_string())
        .collect();
    assert!(
        hard.is_empty(),
        "campaign alias shape must resolve, got {hard:?}"
    );
}

#[test]
fn reports_type_alias_cycle() {
    compile_and_expect("typus A = B\n typus B = A", "SEM007", "type_alias_cycle");
}

#[test]
fn reports_alias_lowering_error_with_structured_event() {
    let diagnostics = compile_and_expect("typus Bad = lista", "SEM008", "lista_arity");
    let err = expect_diagnostic(&diagnostics, "SEM008", "lista_arity");
    assert!(err.args.iter().all(|arg| arg.name != "collection"));
}

#[test]
fn resolve_test_reports_tabula_arity() {
    compile_and_expect("typus Bad = tabula", "SEM008", "tabula_arity");
}

fn resolve_bounded_source(source: &str) -> (TypeTable, Result<(), Vec<crate::SemanticError>>) {
    let parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    let resolved = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);
    (types, resolved)
}

#[test]
fn resolve_test_proposed_bounded_lista_typechecks() {
    let (_types, resolved) = resolve_bounded_source("typus X = lista<numerus, 8>");
    resolved.unwrap_or_else(|errors| panic!("lista<numerus, 8> must resolve, got {errors:?}"));
}

#[test]
fn resolve_test_proposed_bounded_cords_typecheck() {
    for source in [
        "typus X = textus<8>",
        "typus X = ascii<8>",
        "typus X = octeti<8>",
    ] {
        let (_types, resolved) = resolve_bounded_source(source);
        resolved.unwrap_or_else(|errors| panic!("{source} must resolve, got {errors:?}"));
    }
}

#[test]
fn reports_unknown_type_with_structured_event() {
    let parsed = parse(lex("functio f(Missing value) → vacuum {}"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");

    let errors = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types)
        .expect_err("resolve should fail");

    expect_structured_error(&errors, SemanticErrorKind::TypeUndefined, "unknown_type");
}

#[test]
fn reports_invalid_width_marker_with_structured_event() {
    let cases = [
        (
            "functio f(numerus<wat> value) → vacuum {}",
            "invalid_numerus_width_marker",
            "numerus",
        ),
        (
            "functio f(fractus<wat> value) → vacuum {}",
            "invalid_fractus_width_marker",
            "fractus",
        ),
    ];

    for (source, issue, primitive) in cases {
        let parsed = parse(lex(source));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let mut resolver = Resolver::new();
        let mut types = TypeTable::new();
        let program = parsed.program.as_ref().expect("program");
        collect::collect(program, &mut resolver, &mut types).expect("collect");

        let errors = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types)
            .expect_err("resolve should fail");
        let err = expect_structured_error(&errors, SemanticErrorKind::TypeUndefined, issue);

        assert!(
            err.args
                .iter()
                .any(|arg| arg.name == "primitive" && arg.value == primitive)
        );
    }
}

// NOTE: This test must build the AST manually because `tensor<numerus>` (named
// type without figura) is rejected by the parser as a parse error before the
// resolve pass would otherwise run. The test name and issue are preserved to
// document this lowerer invariant.
#[test]
fn reports_incomplete_tensor_type_with_structured_event() {
    let mut interner = Interner::new();
    let tensor = ident(&mut interner, "tensor");
    let numerus = named_type(ident(&mut interner, "numerus"));
    let program =
        func_with_param_type(&mut interner, named_type_with_params(tensor, vec![numerus]));

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(&program, &mut resolver, &mut types).expect("collect");

    let errors = resolve::resolve(&program, &mut resolver, &interner, &mut types)
        .expect_err("resolve should fail");

    expect_structured_error(
        &errors,
        SemanticErrorKind::TypeMismatch,
        "tensor_missing_figura",
    );
}

#[test]
fn reports_non_index_figura_name_with_structured_event() {
    compile_and_expect(
        "functio f<T>(tensor<numerus, T> value) → vacuum {}",
        "SEM010",
        "figura_not_index_param",
    );
}

#[test]
fn imports_function_export_before_struct_alphabetically() {
    // Regression: triga-style libraries export functions that name genera; BTreeMap
    // export order must not fail NominalTypeUnsupported for later-sorted structs.
    use crate::file_interface::{
        InterfaceCallable, InterfaceNominalKind, InterfaceNominalType, InterfaceParam,
        InterfaceTypeSnapshot,
    };
    use crate::{Primitive, SemanticParamMode};

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let mut interface = FileInterface::new();
    // "matrix4_applica" sorts before "Vector3" would if mixed — functions first is the hazard.
    interface.insert(FileExport {
        name: "applica".to_owned(),
        kind: FileExportKind::Function(InterfaceCallable {
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            params: vec![InterfaceParam {
                ty: InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
                    kind: InterfaceNominalKind::Struct,
                    name: "Vector3".to_owned(),
                    identity: None,
                    kind_guessed: false,
                }),
                mode: SemanticParamMode::Ref,
                optional: false,
            }],
            ret: Box::new(InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
                kind: InterfaceNominalKind::Struct,
                name: "Vector3".to_owned(),
                identity: None,
                kind_guessed: false,
            })),
            err: None,
            is_async: false,
            is_generator: false,
            device: None,
        }),
    });
    interface.insert(FileExport {
        name: "Vector3".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: "Vector3".to_owned(),
            fields: vec![crate::file_interface::InterfaceStructField {
                name: "x".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Fractus),
                optional: false,
                required: true,
                default: None,
            }],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });

    resolver
        .set_file_interfaces_from_snapshots(
            &std::collections::BTreeMap::from([("triga".to_owned(), interface)]),
            &mut interner,
            &mut types,
        )
        .expect("file interface import must succeed with two-phase nominal install");

    let ns = interner.intern("triga");
    let member = interner.intern("Vector3");
    assert!(resolver.namespace_file_type(ns, member).is_some());
    let applica = interner.intern("applica");
    assert!(resolver.namespace_file_function(ns, applica).is_some());
}

#[test]
fn projects_localized_library_members_by_identity_and_rejects_unmapped_canonical() {
    let identity = InterfaceLibraryIdentity {
        provider: "faber".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["math".to_owned()],
    };
    let callable = || InterfaceCallable {
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
        err: None,
        is_async: false,
        is_generator: false,
        device: None,
    };
    let mut interface = FileInterface::new().with_identity(identity.clone());
    interface.insert(FileExport {
        name: "summa".to_owned(),
        kind: FileExportKind::Function(callable()),
    });
    interface.insert(FileExport {
        name: "Vector".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: "Vector".to_owned(),
            fields: Vec::new(),
            methods: vec![InterfaceMethodExport {
                name: "length".to_owned(),
                callable: callable(),
            }],
            annotation_contract: None,
        }),
    });
    interface.insert(FileExport {
        name: "PI".to_owned(),
        kind: FileExportKind::Const(InterfaceTypeSnapshot::Primitive(Primitive::Fractus)),
    });

    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "en-test"
schema_version = 1
fallback = ["la"]

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["math"]
kind = "function"
canonical = "summa"
surface = "sum"

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["math"]
kind = "type"
canonical = "Vector"
surface = "VectorEn"

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["math"]
owner = "Vector"
kind = "method"
canonical = "length"
surface = "longitudo"

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["math"]
kind = "const"
canonical = "PI"
surface = "pi"

[llm]
system_prompt_snippet = "Emit test source"
exemplars = ["test.fab"]
"#,
    )
    .expect("library-member locale pack");

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    resolver
        .set_file_interfaces_from_snapshots_with_locale(
            &std::collections::BTreeMap::from([("math".to_owned(), interface)]),
            &mut interner,
            &mut types,
            Some(&pack),
        )
        .expect("localized file interface import");

    let namespace = interner.intern("math");
    assert!(
        resolver
            .namespace_file_function(namespace, interner.intern("sum"))
            .is_some()
    );
    assert!(
        resolver
            .namespace_file_function(namespace, interner.intern("summa"))
            .is_none(),
        "unmapped canonical spelling must not resolve under a non-la pack that defines the identity"
    );
    assert!(
        resolver
            .namespace_file_type(namespace, interner.intern("VectorEn"))
            .is_some()
    );
    assert!(
        resolver
            .namespace_file_type(namespace, interner.intern("Vector"))
            .is_none()
    );
    assert!(
        resolver
            .namespace_file_type(namespace, interner.intern("pi"))
            .is_some()
    );

    let vector = resolver
        .imported_file_type_exports(namespace)
        .into_iter()
        .find(|export| export.member == interner.intern("VectorEn"))
        .expect("localized Vector export")
        .def_id;
    assert_eq!(
        resolver.imported_method_canonical_name(vector, interner.intern("longitudo")),
        Some(interner.intern("length"))
    );
    assert_eq!(
        resolver.imported_method_canonical_name(vector, interner.intern("length")),
        Some(interner.intern("length"))
    );
    assert_eq!(
        pack.library_member_surface(&identity, None, InterfaceMemberKind::Function, "summa"),
        Some("sum")
    );
}

#[test]
fn skipped_locale_members_keep_structured_provenance_and_sibling_exports() {
    let identity = InterfaceLibraryIdentity {
        provider: "faber".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["math".to_owned()],
    };
    let poison = || {
        InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
            kind: InterfaceNominalKind::Struct,
            name: "Missing".to_owned(),
            identity: Some(InterfaceLibraryIdentity {
                provider: "other".to_owned(),
                package: Some("missing".to_owned()),
                module_path: vec!["types".to_owned()],
            }),
            kind_guessed: false,
        })
    };
    let callable = || InterfaceCallable {
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
        err: None,
        is_async: false,
        is_generator: false,
        device: None,
    };
    let mut interface = FileInterface::new().with_identity(identity);
    interface.insert(FileExport {
        name: "Thing".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: "Thing".to_owned(),
            fields: vec![InterfaceStructField {
                name: "bad_field".to_owned(),
                ty: poison(),
                optional: false,
                required: true,
                default: None,
            }],
            methods: vec![
                InterfaceMethodExport {
                    name: "good_method".to_owned(),
                    callable: callable(),
                },
                InterfaceMethodExport {
                    name: "omitted_method".to_owned(),
                    callable: callable(),
                },
            ],
            annotation_contract: None,
        }),
    });
    interface.insert(FileExport {
        name: "salve".to_owned(),
        kind: FileExportKind::Function(callable()),
    });

    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "en-test"
schema_version = 1
fallback = ["la"]

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["math"]
kind = "type"
canonical = "Thing"
surface = "ThingEn"

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["math"]
kind = "function"
canonical = "salve"
surface = "salve"

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["math"]
owner = "Thing"
kind = "field"
canonical = "bad_field"
surface = "bad_field_en"

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["math"]
owner = "Thing"
kind = "method"
canonical = "good_method"
surface = "good_method_en"

[llm]
system_prompt_snippet = "Emit test source"
exemplars = ["test.fab"]
"#,
    )
    .expect("library-member locale pack");

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let skipped = resolver
        .set_file_interfaces_from_snapshots_with_locale(
            &std::collections::BTreeMap::from([("math".to_owned(), interface)]),
            &mut interner,
            &mut types,
            Some(&pack),
        )
        .expect("localized file interface import");

    let namespace = interner.intern("math");
    assert!(
        resolver
            .namespace_file_function(namespace, interner.intern("salve"))
            .is_some()
    );
    assert!(
        resolver
            .namespace_file_type(namespace, interner.intern("ThingEn"))
            .is_some()
    );
    assert!(
        resolver
            .namespace_file_type(namespace, interner.intern("Thing"))
            .is_none()
    );

    let bad_field = skipped
        .iter()
        .find(|skip| skip.canonical_name == "bad_field")
        .expect("field record");
    assert_eq!(bad_field.provider, "faber");
    assert_eq!(bad_field.package, "demo");
    assert_eq!(bad_field.module, "math");
    assert_eq!(bad_field.owner, "Thing");
    assert_eq!(bad_field.kind, "field");
    assert_eq!(bad_field.projected_surfaces, vec!["bad_field_en"]);
    assert!(bad_field.remap_reason.contains("NominalIdentityUnknown"));

    let omitted = skipped
        .iter()
        .find(|skip| skip.canonical_name == "omitted_method")
        .expect("omitted record");
    assert_eq!(omitted.owner, "Thing");
    assert_eq!(omitted.kind, "method");
    assert!(omitted.projected_surfaces.is_empty());
    assert_eq!(omitted.remap_reason, "strict_locale_pack_member_omitted");

    let thing = resolver
        .imported_file_structs_for_namespace(namespace)
        .into_iter()
        .next()
        .expect("Thing");
    assert!(thing.1.is_empty(), "poison field must not remain installed");
    assert_eq!(thing.2.len(), 1);
    assert_eq!(thing.2[0].surface_name, interner.intern("good_method_en"));
}

#[test]
fn identity_row_library_members_resolve_under_strict_pack() {
    // A non-la pack declares the identity rows (surface == canonical) for a
    // library it defines as total. Identity rows must install exactly like
    // non-identity rows: the member resolves at its canonical spelling
    // (which the identity row accepts), never SEM004 namespace_missing_export.
    let identity = InterfaceLibraryIdentity {
        provider: "faber".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["corda".to_owned()],
    };
    let callable = || InterfaceCallable {
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
        err: None,
        is_async: false,
        is_generator: false,
        device: None,
    };
    let mut interface = FileInterface::new().with_identity(identity.clone());
    interface.insert(FileExport {
        name: "pange".to_owned(),
        kind: FileExportKind::Function(callable()),
    });
    interface.insert(FileExport {
        name: "solve".to_owned(),
        kind: FileExportKind::Function(callable()),
    });
    interface.insert(FileExport {
        name: "retorta".to_owned(),
        kind: FileExportKind::Function(callable()),
    });

    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "en-test"
schema_version = 1
fallback = ["la"]

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["corda"]
kind = "function"
canonical = "pange"
surface = "pange"

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["corda"]
kind = "function"
canonical = "solve"
surface = "solve"

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["corda"]
kind = "function"
canonical = "retorta"
surface = "reverse"

[[library_members]]
provider = "faber"
package = "demo"
module_path = ["corda"]
kind = "function"
canonical = "retorta"
surface = "retorta"

[llm]
system_prompt_snippet = "Emit test source"
exemplars = ["test.fab"]
"#,
    )
    .expect("identity-row library-member locale pack");

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    resolver
        .set_file_interfaces_from_snapshots_with_locale(
            &std::collections::BTreeMap::from([("corda".to_owned(), interface)]),
            &mut interner,
            &mut types,
            Some(&pack),
        )
        .expect("localized identity-row file interface import");

    let namespace = interner.intern("corda");
    for member in ["pange", "solve", "reverse", "retorta"] {
        assert!(
            resolver
                .namespace_file_function(namespace, interner.intern(member))
                .is_some(),
            "identity-row member `{member}` must resolve under the strict pack"
        );
        assert!(
            resolver.namespace_member_surface_valid(namespace, interner.intern(member)),
            "identity-row surface `{member}` must be accepted at the source boundary"
        );
    }
    assert_eq!(
        resolver.imported_export_canonical_name(namespace, interner.intern("reverse")),
        interner.intern("retorta"),
        "non-identity row still projects surface to canonical for HIR"
    );
    assert_eq!(
        resolver.imported_export_canonical_name(namespace, interner.intern("pange")),
        interner.intern("pange"),
        "identity row keeps the canonical spelling"
    );
    assert_eq!(
        resolver.imported_export_surface_name(namespace, interner.intern("retorta")),
        interner.intern("reverse"),
        "translated surface remains primary when an identity alias is also declared"
    );
}

#[test]
fn imports_nominal_fields_across_namespaces_before_payloads() {
    let mut scene = FileInterface::new();
    scene.insert(FileExport {
        name: "SceneNode".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: "SceneNode".to_owned(),
            fields: vec![InterfaceStructField {
                name: "local_matrix".to_owned(),
                ty: InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
                    kind: InterfaceNominalKind::Struct,
                    name: "Matrix4".to_owned(),
                    identity: None,
                    kind_guessed: false,
                }),
                optional: false,
                required: true,
                default: None,
            }],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });

    let mut triga = FileInterface::new();
    triga.insert(FileExport {
        name: "Matrix4".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: "Matrix4".to_owned(),
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &std::collections::BTreeMap::from([
                ("scene".to_owned(), scene),
                ("triga".to_owned(), triga),
            ]),
            &mut interner,
            &mut types,
        )
        .expect("cross-namespace nominal field must import");

    let scene_structs = resolver.imported_file_structs_for_namespace(interner.intern("scene"));
    assert_eq!(scene_structs.len(), 1);
    assert_eq!(scene_structs[0].1.len(), 1);
    assert_eq!(
        scene_structs[0].1[0].ty,
        resolver
            .namespace_file_type(interner.intern("triga"), interner.intern("Matrix4"))
            .expect("Matrix4 shell")
    );
}

#[test]
fn ambiguous_nominal_fields_do_not_bind_to_last_namespace() {
    let mut scene = FileInterface::new();
    scene.insert(FileExport {
        name: "SceneNode".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: "SceneNode".to_owned(),
            fields: vec![InterfaceStructField {
                name: "shape".to_owned(),
                ty: InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
                    kind: InterfaceNominalKind::Struct,
                    name: "Shape".to_owned(),
                    identity: None,
                    kind_guessed: false,
                }),
                optional: false,
                required: true,
                default: None,
            }],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });

    let mut first = FileInterface::new();
    first.insert(FileExport {
        name: "Shape".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: "Shape".to_owned(),
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    let mut second = FileInterface::new();
    second.insert(FileExport {
        name: "Shape".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: "Shape".to_owned(),
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &std::collections::BTreeMap::from([
                ("scene".to_owned(), scene),
                ("first".to_owned(), first),
                ("second".to_owned(), second),
            ]),
            &mut interner,
            &mut types,
        )
        .expect("ambiguous nominal names should remain a recoverable interface import");

    let shape = interner.intern("Shape");
    assert!(resolver.imported_nominal_type_is_ambiguous(InterfaceNominalKind::Struct, shape));
    let scene_structs = resolver.imported_file_structs_for_namespace(interner.intern("scene"));
    assert_eq!(scene_structs.len(), 1);
    assert!(
        scene_structs[0].1.is_empty(),
        "ambiguous field must not bind to an arbitrary namespace"
    );
}

#[test]
fn resolves_qualified_type_through_file_interface() {
    let parsed = parse(lex(r#"
importa ex "./terminus" terminus
functio f(terminus.Terminus value) → vacuum {}
"#));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Terminus".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: "Terminus".to_owned(),
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    let mut interface_interner = parsed.interner.clone();
    resolver
        .set_file_interfaces_from_snapshots(
            &std::collections::BTreeMap::from([("terminus".to_owned(), interface)]),
            &mut interface_interner,
            &mut types,
        )
        .expect("install file interface");

    let result = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);

    assert!(result.is_ok(), "resolve errors: {result:?}");
}

#[test]
fn rejects_qualified_type_prefix_that_is_not_module_binding() {
    let parsed = parse(lex(r"
genus terminus {}
genus Terminus {}
functio f(terminus.Terminus value) → vacuum {}
"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");

    let errors = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types)
        .expect_err("resolve should fail");

    expect_structured_error(
        &errors,
        SemanticErrorKind::TypeUndefined,
        "qualified_type_prefix_not_namespace",
    );
}

#[test]
fn type_alias_union_deduplicates_structurally_equal_compound_members() {
    let source = "typus Numerorum = lista<numerus> ∪ lista<numerus>";
    let mut parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(program, &mut resolver, &mut types);
    let result = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);
    assert!(result.is_ok());

    let alias_sym = parsed.interner.intern("Numerorum");
    let def_id = resolver.lookup(alias_sym).expect("alias def");
    let symbol = resolver.get_symbol(def_id).expect("alias symbol");
    let alias_ty = symbol.ty.expect("alias type");
    assert!(matches!(types.get(alias_ty), Type::Array(_)));
}

#[test]
fn type_alias_union_dedups_alias_with_peeled_target_member() {
    // `typus Id = numerus`: the alias and the type it resolves to are distinct
    // arena entries, so `Id ∪ numerus` must collapse to a single member through
    // the canonical sort key — otherwise the union carries a redundant member.
    let source = "typus Id = numerus\ntypus Numerorum = Id ∪ numerus";
    let mut parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(program, &mut resolver, &mut types);
    let result = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);
    assert!(result.is_ok());

    let alias_sym = parsed.interner.intern("Numerorum");
    let def_id = resolver.lookup(alias_sym).expect("alias def");
    let symbol = resolver.get_symbol(def_id).expect("alias symbol");
    let alias_ty = symbol.ty.expect("alias type");
    assert!(
        types.equals(alias_ty, types.primitive(Primitive::Numerus)),
        "collapsed union must resolve to its single peeled member, got {:?}",
        types.get(alias_ty)
    );
}

#[test]
fn type_alias_union_dedups_alias_even_when_not_typeid_adjacent() {
    // Same raw-id sort shape as the union repair: `dedup_by(equals)` only
    // removes *adjacent* equals-equal members. With an unrelated member (`L`)
    // sorted between the alias (`Id`) and its target (`numerus`) on raw
    // `TypeId`, both survive unless the sort key clusters equals-true members
    // first. `L`'s alias is interned before `Id`'s, so the raw ordinals are
    // `numerus < L < Id` — the exact defect class.
    let source = "typus L = textus\ntypus Id = numerus\ntypus Numerorum = L ∪ Id ∪ numerus";
    let mut parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(program, &mut resolver, &mut types);
    let result = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);
    assert!(result.is_ok());

    let alias_sym = parsed.interner.intern("Numerorum");
    let def_id = resolver.lookup(alias_sym).expect("alias def");
    let symbol = resolver.get_symbol(def_id).expect("alias symbol");
    let alias_ty = symbol.ty.expect("alias type");
    let Type::Union(members) = types.get(alias_ty) else {
        panic!("expected union, got {:?}", types.get(alias_ty));
    };
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    assert_eq!(
        members.len(),
        2,
        "alias member must collapse with its peeled target, got {members:?}"
    );
    assert!(members.iter().any(|member| types.equals(*member, numerus)));
    assert!(members.iter().any(|member| types.equals(*member, textus)));
}

#[test]
fn type_alias_union_spellings_with_and_without_alias_member_are_equal() {
    // After canonical dedup, the alias spelling `numerus ∪ L ∪ Id` and the
    // direct spelling `numerus ∪ L` resolve to the identical union (the alias
    // member is redundant), so the two resolved types must be equal.
    let source = "typus L = textus\ntypus Id = numerus\ntypus Numerorum = numerus ∪ L ∪ Id\ntypus NumerorumAliter = numerus ∪ L";
    let mut parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.as_ref().expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(program, &mut resolver, &mut types);
    let result = resolve::resolve(program, &mut resolver, &parsed.interner, &mut types);
    assert!(result.is_ok());

    let with_alias_sym = parsed.interner.intern("Numerorum");
    let with_alias_def = resolver.lookup(with_alias_sym).expect("alias def");
    let with_alias_ty = resolver
        .get_symbol(with_alias_def)
        .expect("alias symbol")
        .ty
        .expect("alias type");

    let direct_sym = parsed.interner.intern("NumerorumAliter");
    let direct_def = resolver.lookup(direct_sym).expect("alias def");
    let direct_ty = resolver
        .get_symbol(direct_def)
        .expect("alias symbol")
        .ty
        .expect("alias type");

    assert!(
        types.equals(with_alias_ty, direct_ty),
        "alias spelling and direct spelling of the same union must resolve to equal unions: {:?} vs {:?}",
        types.get(with_alias_ty),
        types.get(direct_ty)
    );
}

#[test]
fn reports_non_variant_pattern() {
    compile_and_expect(
        "ordo MyEnum { Red }\n functio NotAVariant() → vacuum {}\n discerne 1 { casu MyEnum.NotAVariant {} }",
        "SEM001",
        "name_not_variant",
    );
}

#[test]
fn reports_unknown_variant_with_structured_event() {
    compile_and_expect(
        "ordo MyEnum { Red, Green }\n discerne 1 { casu MyEnum.Missing {} }",
        "SEM001",
        "unknown_variant",
    );
}

#[test]
fn resolves_qualified_variant_through_parent_despite_local_shadow() {
    let result =
        compile_ok("ordo Color { Red }\n incipit { fixum numerus Red ← 1 nota Color.Red }");

    assert!(
        result
            .diagnostics
            .iter()
            .all(|diag| diag.issue() != Some("unknown_variant")
                && diag.issue() != Some("name_not_variant")),
        "qualified variant should use enum parent map, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (crate::semantic_spec(diag.kind).code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn allows_variant_patterns_from_collected_enum() {
    let result = compile_ok("ordo Color { Red, Green }\n discerne 1 { casu Color.Red {} }");
    assert!(
        result.is_ok(),
        "enum variant pattern should resolve: {:?}",
        result.diagnostics
    );
}

#[test]
fn allows_qualified_variant_expression_from_owning_enum() {
    let result = compile_ok("ordo A { One }\nincipit { nota A.One }");
    assert!(
        result.is_ok(),
        "owning enum variant expression should resolve: {:?}",
        result.diagnostics
    );
}

#[test]
fn rejects_qualified_variant_expression_from_other_enum() {
    compile_and_expect(
        "ordo A { One }\nordo B { Two }\nincipit { nota A.Two }",
        "SEM001",
        "unknown_variant",
    );
}

#[test]
fn cross_union_same_name_variants_declare_and_resolve_qualified() {
    // uvn-u2: two unions sharing a variant name declare cleanly and `A.X` /
    // `B.X` are distinct identities; qualified values and qualified elige
    // casu patterns compile.
    let result = compile_ok(
        "discretio A { X }\ndiscretio B { X }\n\
         incipit {\n  fixum _ ax ← A.X\n  fixum _ bx ← B.X\n\
         \x20 elige ax { casu A.X { nota \"a\" } }\n\
         \x20 elige bx { casu B.X { nota \"b\" } }\n}",
    );
    assert!(
        result.is_ok(),
        "cross-union same-name variants should compile: {:?}",
        result.diagnostics
    );
}

#[test]
fn same_union_duplicate_variant_still_sem005() {
    compile_and_expect("discretio A { X, X }", "SEM005", "duplicate_definition");
}

#[test]
fn ambiguous_bare_variant_expression_reports_variant_ambiguous() {
    compile_and_expect(
        "discretio A { X }\ndiscretio B { X }\nincipit { fixum _ x ← X }",
        "SEM001",
        "variant_ambiguous",
    );
}

#[test]
fn ambiguous_bare_variant_finge_reports_variant_ambiguous() {
    compile_and_expect(
        "discretio A { X { numerus v } }\ndiscretio B { X { numerus v } }\n\
         functio f() → A { redde finge X { v = 1 } }",
        "SEM001",
        "variant_ambiguous",
    );
}

#[test]
fn single_union_bare_variant_surfaces_unchanged() {
    // uvn-u2 regression: single-union bare variant use in finge, discerne
    // casu arms, and expression position resolves exactly as before the
    // variant identity re-key.
    let result = compile_ok(
        "discretio Nodus {\n  Groupus { lista<numerus> filii },\n  Tessera { numerus geometria }\n}\n\
         functio make(numerus g) → Nodus {\n  redde finge Tessera { geometria = g } ∷ Nodus\n}\n\
         functio read(Nodus n) → numerus {\n  discerne n {\n\
         \x20   casu Tessera fixum geometria { redde geometria }\n\
         \x20   casu Groupus fixum filii { redde 0 }\n  }\n}\n\
         incipit {\n  fixum Nodus n ← make(7)\n  nota read(n)\n}",
    );
    assert!(
        result.is_ok(),
        "single-union bare variant use should compile: {:?}",
        result.diagnostics
    );
}

#[test]
fn rejects_qualified_construction_with_unknown_prefix() {
    // CG2-R07: a leading path segment that is not a real qualifier must
    // reject — `finge missing.A.X { }` must never silently resolve as the
    // local `A.X`.
    compile_and_expect(
        "discretio A { X }\nincipit { fixum _ v ← finge missing.A.X { } ∷ A }",
        "SEM001",
        "unknown_variant",
    );
}

#[test]
fn rejects_qualified_construction_with_non_namespace_prefix() {
    // CG2-R07: the leading path segment must resolve as a real qualifier —
    // a value/type binding in prefix position is not a namespace, so the
    // path must reject rather than resolve the penultimate local union.
    compile_and_expect(
        "discretio A { X }\nincipit { fixum numerus missing ← 1\n fixum _ v ← finge missing.A.X { } ∷ A }",
        "SEM001",
        "unknown_variant",
    );
}

#[test]
fn allows_exact_local_qualified_construction() {
    // CG2-R07 regression pin: the exact local qualifier `finge A.X { }`
    // still resolves through the local union's parent.
    let result = compile_ok("discretio A { X }\nincipit { fixum _ v ← finge A.X { } ∷ A }");
    assert!(
        result.is_ok(),
        "exact local qualified construction should resolve: {:?}",
        result.diagnostics
    );
}

#[test]
fn reports_break_outside_breakable_block() {
    let diagnostics = compile_and_expect("rumpe", "SEM030", "break_outside_breakable");
    let err = expect_diagnostic(&diagnostics, "SEM030", "break_outside_breakable");
    assert!(err.is_error());
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "keyword" && arg.value == "rumpe")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "context" && arg.value == "breakable block")
    );
}

#[test]
fn reports_continue_outside_loop() {
    let diagnostics = compile_and_expect("perge", "SEM031", "continue_outside_loop");
    let err = expect_diagnostic(&diagnostics, "SEM031", "continue_outside_loop");
    assert!(err.is_error());
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "keyword" && arg.value == "perge")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "context" && arg.value == "loop")
    );
}

#[test]
fn accepts_rumpe_inside_fac_body() {
    let source = r"
functio exemplo() → numerus {
    fac {
        rumpe
    } cape err {
        redde 0
    }
    redde 1
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    assert!(
        parse_result.success(),
        "parse failed: {:?}",
        parse_result
            .errors
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>()
    );
    let program = parse_result.program.expect("parsed program");
    let interner = parse_result.interner;

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(&program, &mut resolver, &mut types);
    let result = resolve::resolve(&program, &mut resolver, &interner, &mut types);
    assert!(
        result.is_ok(),
        "rumpe inside fac should resolve: {result:?}"
    );
}

#[test]
fn accepts_perge_inside_fac_body() {
    let source = r"
functio exemplo(bivalens condicio) → numerus {
    fac {
        perge
        redde 0
    } dum condicio
    redde 1
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    assert!(
        parse_result.success(),
        "parse failed: {:?}",
        parse_result
            .errors
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>()
    );
    let program = parse_result.program.expect("parsed program");
    let interner = parse_result.interner;

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(&program, &mut resolver, &mut types);
    let result = resolve::resolve(&program, &mut resolver, &interner, &mut types);
    assert!(
        result.is_ok(),
        "perge inside fac…dum should resolve: {result:?}"
    );
}

#[test]
fn accepts_perge_inside_fac_without_dum() {
    let source = r"
functio exemplo() → numerus {
    fac {
        perge
    }
    redde 1
}
";
    let lex_result = radix_lexer::lex(source);
    let parse_result = radix_parser::parse(lex_result);
    assert!(
        parse_result.success(),
        "parse failed: {:?}",
        parse_result
            .errors
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>()
    );
    let program = parse_result.program.expect("parsed program");
    let interner = parse_result.interner;

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let _ = collect::collect(&program, &mut resolver, &mut types);
    let result = resolve::resolve(&program, &mut resolver, &interner, &mut types);
    assert!(
        result.is_ok(),
        "perge inside fac should resolve: {result:?}"
    );
}

fn resolve_source(source: &str) -> Vec<crate::SemanticError> {
    let parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    resolve::resolve(program, &mut resolver, &parsed.interner, &mut types)
        .err()
        .unwrap_or_default()
}

#[test]
fn warns_on_unrecognized_curata_allocator() {
    let errors = resolve_source("functio f() curata evil → numerus { redde 0 }");

    let err = expect_structured_error(
        &errors,
        SemanticErrorKind::Warning(crate::error::WarningKind::UnrecognizedCurataAllocator),
        "unrecognized_curata_allocator",
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "name" && arg.value == "evil")
    );
    assert!(!err.is_error(), "curata lint must stay warning-level");
}

#[test]
fn allows_recognized_curata_allocators() {
    for name in ["arena", "page", "alloc", "allocator"] {
        let source = format!("functio f() curata {name} → numerus {{ redde 0 }}");
        let errors = resolve_source(&source);
        assert!(
            errors.is_empty(),
            "recognized allocator '{name}' should not warn: {errors:?}"
        );
    }
}

#[test]
fn resolves_empty_program() {
    let result = compile_ok("");
    assert!(
        result.is_ok(),
        "empty program must resolve: {:?}",
        result.diagnostics
    );
}

#[test]
fn resolves_program_with_only_incipit_and_nota() {
    let result = compile_ok("incipit { nota 42 }");
    assert!(
        result.is_ok(),
        "trivial incipit must resolve: {:?}",
        result.diagnostics
    );
}

#[test]
fn resolves_empty_function_body() {
    let result = compile_ok("functio f() → vacuum {}");
    assert!(
        result.is_ok(),
        "empty function body must resolve: {:?}",
        result.diagnostics
    );
}

/// S1 U2 canonical nominal identity: same-name nominals from distinct module
/// identities stay distinct (both resolve canonically), while the same module
/// installed under two aliases unifies to ONE TypeId.
#[test]
fn canonical_nominal_identity_distinguishes_modules_and_unifies_aliases() {
    let util_identity = InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["util".to_owned()],
    };
    let other_identity = InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["other".to_owned()],
    };
    let nominal_export = |name: &str| FileExport {
        name: name.to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            params: Vec::new(),
            name: name.to_owned(),
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    };
    let util = FileInterface::new().with_identity(util_identity.clone());
    let util = {
        let mut i = util;
        i.insert(nominal_export("Persona"));
        i
    };
    let other = FileInterface::new().with_identity(other_identity.clone());
    let other = {
        let mut i = other;
        i.insert(nominal_export("Persona"));
        i
    };

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    // The same module under two aliases (`utilModule`, `aliasModule`) plus a
    // distinct module also exporting `Persona`.
    resolver
        .set_file_interfaces_from_snapshots(
            &std::collections::BTreeMap::from([
                ("utilModule".to_owned(), util.clone()),
                ("aliasModule".to_owned(), util),
                ("otherModule".to_owned(), other),
            ]),
            &mut interner,
            &mut types,
        )
        .expect("canonical nominal identities must install without ambiguity");

    let persona = interner.intern("Persona");

    // Distinct module identities → distinct TypeIds, each canonically
    // resolvable (no name-keyed ambiguity on the canonical path).
    let util_ty = resolver
        .imported_nominal_type_by_identity(
            InterfaceNominalKind::Struct,
            Some(&util_identity),
            persona,
        )
        .expect("util Persona resolves canonically");
    let other_ty = resolver
        .imported_nominal_type_by_identity(
            InterfaceNominalKind::Struct,
            Some(&other_identity),
            persona,
        )
        .expect("other Persona resolves canonically");
    assert_ne!(
        util_ty, other_ty,
        "same-name nominals from distinct module identities must not unify"
    );

    // Same module under two aliases → the SAME TypeId (two-aliases-unify).
    let alias_ty = resolver
        .imported_nominal_type_by_identity(
            InterfaceNominalKind::Struct,
            Some(&util_identity),
            persona,
        )
        .expect("alias Persona resolves canonically");
    assert_eq!(
        util_ty, alias_ty,
        "same module under two aliases must unify"
    );
    assert_eq!(
        resolver.imported_nominal_def_id(
            InterfaceNominalKind::Struct,
            Some(&util_identity),
            persona
        ),
        resolver.imported_nominal_def_id(
            InterfaceNominalKind::Struct,
            Some(&util_identity),
            persona
        ),
        "canonical def id is stable"
    );
}

/// uvn-u3 oracle: the five Rivus pilot names (`InvalidAnnotation`,
/// `InvalidAssignmentTarget`, `Warning`, `Lex`, `Parse`) compile green across
/// distinct unions in one module using qualified references — no renames.
/// Mirrors the rivus `src/diagnostics/diagnostic.fab` union set on the
/// pre-rename surface; if a qualified reference resolved to the wrong union's
/// variant, the `redde` return would fail typecheck.
#[test]
fn rivus_pilot_names_compile_qualified_without_renames() {
    let result = compile_ok(
        "discretio Severity { Error, Warning, Info }\n\
         discretio DiagnosticPhase { Io, Lex, Parse, Resolve, Lower, Typecheck, Analysis, Mir, Codegen, Tool }\n\
         discretio ParseErrorKind { LexError, InvalidAnnotation, InvalidAssignmentTarget }\n\
         discretio SemanticErrorKind { UndefinedVariable, DuplicateDefinition, InvalidAnnotation, InvalidAssignmentTarget, Warning }\n\
         discretio PhaseErrorKind { Lex, Parse, Semantic }\n\
         functio severity_warning() → Severity { redde Severity.Warning }\n\
         functio phase_lex() → DiagnosticPhase { redde DiagnosticPhase.Lex }\n\
         functio phase_parse() → DiagnosticPhase { redde DiagnosticPhase.Parse }\n\
         functio parse_annotation() → ParseErrorKind { redde ParseErrorKind.InvalidAnnotation }\n\
         functio parse_assignment_target() → ParseErrorKind { redde ParseErrorKind.InvalidAssignmentTarget }\n\
         functio semantic_annotation() → SemanticErrorKind { redde SemanticErrorKind.InvalidAnnotation }\n\
         functio semantic_assignment_target() → SemanticErrorKind { redde SemanticErrorKind.InvalidAssignmentTarget }\n\
         functio semantic_warning() → SemanticErrorKind { redde SemanticErrorKind.Warning }\n\
         functio phase_error_lex() → PhaseErrorKind { redde PhaseErrorKind.Lex }\n\
         functio phase_error_parse() → PhaseErrorKind { redde PhaseErrorKind.Parse }\n\
         functio phase_error_semantic() → PhaseErrorKind { redde PhaseErrorKind.Semantic }",
    );
    assert!(
        result.is_ok(),
        "rivus pilot names must compile with qualified references: {:?}",
        result.diagnostics
    );
}

/// uvn-u3 oracle (ambiguity policy): bare use of a name that two rivus
/// diagnostic unions define is the documented fail-closed
/// `variant_ambiguous` error, never a silent pick.
#[test]
fn rivus_pilot_bare_colliding_name_reports_variant_ambiguous() {
    compile_and_expect(
        "discretio Severity { Error, Warning, Info }\n\
         discretio SemanticErrorKind { InvalidAnnotation, InvalidAssignmentTarget, Warning }\n\
         incipit { fixum _ w ← Warning }",
        "SEM001",
        "variant_ambiguous",
    );
    compile_and_expect(
        "discretio DiagnosticPhase { Io, Lex, Parse }\n\
         discretio PhaseErrorKind { Lex, Parse, Semantic }\n\
         incipit { fixum _ l ← Lex }",
        "SEM001",
        "variant_ambiguous",
    );
}

// ── KW-U2: binding-wins for builtin-claimed words ──

fn has_issue(result: &crate::test_support::AnalyzedFixture, code: &str, issue: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|d| crate::semantic_spec(d.kind).code == code && d.issue() == Some(issue))
}

#[test]
fn shadowed_lege_binding_is_the_value_not_the_builtin() {
    let result = compile_ok(
        r#"
incipit {
  fixum textus lege ← "data"
  nota lege
}
"#,
    );
    assert!(
        result.is_ok(),
        "binding-named lege must compile: {:?}",
        result.diagnostics
    );
    assert!(
        !has_issue(&result, "WARN001", "unused_variable"),
        "lege binding must be used, not treated as the builtin: {:?}",
        result.diagnostics
    );
}

#[test]
fn shadowed_lege_fn_is_callable() {
    let result = compile_ok(
        r#"
functio lege(textus path) → textus {
  redde path
}
incipit {
  fixum textus x ← lege("cfg")
  nota x
}
"#,
    );
    assert!(
        result.is_ok(),
        "user fn named lege must be callable: {:?}",
        result.diagnostics
    );
    assert!(
        !has_issue(&result, "SEM012", "callee_not_callable"),
        "lege(\"cfg\") must call the user fn, not the builtin: {:?}",
        result.diagnostics
    );
}

#[test]
fn bound_vacua_is_the_value_beside_a_same_named_binding() {
    // F1: the parser's Vacua claim is only a fallback. A live same-named
    // value binding wins in bare expression position, just as it does for
    // calls and other postfix expression bases.
    let result = compile_ok(
        r#"
incipit {
  fixum textus vacua ← "empty"
  nota vacua
}
"#,
    );
    assert!(
        !has_issue(&result, "SEM014", "vacua_requires_declared_type"),
        "bound vacua must not use the marker: {:?}",
        result.diagnostics
    );
    assert!(
        has_issue(&result, "WARN021", "empty_marker_shadowing"),
        "marker-named binding should warn: {:?}",
        result.diagnostics
    );
}

#[test]
fn unshadowed_lege_stays_the_builtin() {
    let result = compile_ok(
        r"
incipit {
  fixum _ input ← lege
}
",
    );
    assert!(
        result.is_ok(),
        "bare unshadowed lege must remain the builtin: {:?}",
        result.diagnostics
    );
}

#[test]
fn shadowed_scriptum_fn_is_callable() {
    let result = compile_ok(
        r#"
functio scriptum(textus t) → textus {
  redde t
}
incipit {
  fixum textus x ← scriptum("hello")
  nota x
}
"#,
    );
    assert!(
        result.is_ok(),
        "user fn named scriptum must be callable: {:?}",
        result.diagnostics
    );
}

#[test]
fn unshadowed_scriptum_stays_the_builtin() {
    let result = compile_ok(
        r#"
incipit {
  fixum textus x ← scriptum("hello")
  nota x
}
"#,
    );
    assert!(
        result.is_ok(),
        "unshadowed scriptum must remain the builtin: {:?}",
        result.diagnostics
    );
}

fn resolve_ok(source: &str) -> (Program, Resolver, Interner) {
    let parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    collect::collect(&program, &mut resolver, &mut types).expect("collect");
    resolve::resolve(&program, &mut resolver, &parsed.interner, &mut types).expect("resolve");
    (program, resolver, parsed.interner)
}

fn walk_exprs<'a>(program: &'a Program, visit: &mut impl FnMut(&'a Expr)) {
    fn walk_stmt<'a>(stmt: &'a Stmt, visit: &mut impl FnMut(&'a Expr)) {
        match &stmt.kind {
            StmtKind::Var(decl) => {
                if let Some(init) = &decl.init {
                    walk_expr(init, visit);
                }
            }
            StmtKind::Scribe(scribe) => {
                for arg in &scribe.args {
                    walk_expr(arg, visit);
                }
            }
            StmtKind::Incipit(incipit) => match &incipit.body {
                IfBody::Block(block) => {
                    for inner in &block.statements {
                        walk_stmt(inner, visit);
                    }
                }
                IfBody::Ergo(inner) => walk_stmt(inner, visit),
            },
            StmtKind::Block(block) => {
                for inner in &block.statements {
                    walk_stmt(inner, visit);
                }
            }
            StmtKind::Func(func) => {
                if let Some(body) = &func.body {
                    for inner in &body.statements {
                        walk_stmt(inner, visit);
                    }
                }
            }
            StmtKind::Expr(stmt) => walk_expr(&stmt.expr, visit),
            _ => {}
        }
    }
    fn walk_expr<'a>(expr: &'a Expr, visit: &mut impl FnMut(&'a Expr)) {
        visit(expr);
        match &expr.kind {
            ExprKind::Call(call) => {
                walk_expr(&call.callee, visit);
                for arg in &call.args {
                    walk_expr(&arg.value, visit);
                }
            }
            ExprKind::Member(member) => walk_expr(&member.object, visit),
            ExprKind::Index(index) => {
                walk_expr(&index.object, visit);
                walk_expr(&index.index, visit);
            }
            ExprKind::OptionalChain(chain) => {
                walk_expr(&chain.object, visit);
                match &chain.chain {
                    radix_syntax::OptionalChainKind::Member(_) => {}
                    radix_syntax::OptionalChainKind::Index(index) => walk_expr(index, visit),
                    radix_syntax::OptionalChainKind::Call(args) => {
                        for arg in args {
                            walk_expr(&arg.value, visit);
                        }
                    }
                }
            }
            ExprKind::NonNull(chain) => {
                walk_expr(&chain.object, visit);
                match &chain.chain {
                    radix_syntax::NonNullKind::Member(_) => {}
                    radix_syntax::NonNullKind::Index(index) => walk_expr(index, visit),
                    radix_syntax::NonNullKind::Call(args) => {
                        for arg in args {
                            walk_expr(&arg.value, visit);
                        }
                    }
                }
            }
            ExprKind::Paren(inner) => walk_expr(inner, visit),
            ExprKind::Scriptum(scriptum) => {
                for arg in &scriptum.args {
                    walk_expr(arg, visit);
                }
            }
            _ => {}
        }
    }
    for stmt in &program.statements {
        walk_stmt(stmt, visit);
    }
}

#[test]
fn resolve_records_shadowing_for_bound_lege() {
    let (program, resolver, _) = resolve_ok(
        r#"
incipit {
  fixum textus lege ← "data"
  nota lege
}
"#,
    );
    let mut lege_nodes = Vec::new();
    walk_exprs(&program, &mut |expr| {
        if matches!(expr.kind, ExprKind::Lege(_)) {
            lege_nodes.push(expr.id);
        }
    });
    assert_eq!(lege_nodes.len(), 1, "expected one claimed lege node");
    assert!(
        resolver.shadowed_builtin(lege_nodes[0]).is_some(),
        "bound lege must record shadowing"
    );
}

#[test]
fn resolve_does_not_record_shadowing_for_unbound_lege() {
    let (program, resolver, _) = resolve_ok(
        r"
incipit {
  fixum _ input ← lege
}
",
    );
    let mut lege_nodes = Vec::new();
    walk_exprs(&program, &mut |expr| {
        if matches!(expr.kind, ExprKind::Lege(_)) {
            lege_nodes.push(expr.id);
        }
    });
    assert_eq!(lege_nodes.len(), 1, "expected one claimed lege node");
    assert!(
        resolver.shadowed_builtin(lege_nodes[0]).is_none(),
        "unbound lege must stay the builtin"
    );
}

#[test]
fn resolve_records_shadowing_for_bound_vacua() {
    let (program, resolver, _) = resolve_ok(
        r#"
incipit {
  fixum textus vacua ← "empty"
  nota vacua
}
"#,
    );
    let mut vacua_nodes = Vec::new();
    walk_exprs(&program, &mut |expr| {
        if matches!(expr.kind, ExprKind::Vacua(_)) {
            vacua_nodes.push(expr.id);
        }
    });
    assert_eq!(vacua_nodes.len(), 1, "expected one claimed vacua node");
    assert!(
        resolver.shadowed_builtin(vacua_nodes[0]).is_some(),
        "bound vacua must record shadowing"
    );
}

#[test]
fn resolve_records_shadowing_for_vacua_in_call_position() {
    let (program, resolver, _) = resolve_ok(
        r"
functio vacua() → numerus { redde 1 }
incipit {
  fixum _ x ← vacua()
}
",
    );
    let mut vacua_callees = Vec::new();
    walk_exprs(&program, &mut |expr| {
        if let ExprKind::Call(call) = &expr.kind {
            if matches!(call.callee.kind, ExprKind::Vacua(_)) {
                vacua_callees.push(call.callee.id);
            }
        }
    });
    assert_eq!(vacua_callees.len(), 1, "expected one Vacua call callee");
    assert!(
        resolver.shadowed_builtin(vacua_callees[0]).is_some(),
        "vacua() must record shadowing on the callee"
    );
}

#[test]
fn resolve_binding_wins_in_every_marker_expression_position() {
    let (program, resolver, _) = resolve_ok(
        r"
incipit {
  fixum lista<numerus> vacua ← [1, 2, 3]
  nota vacua
  nota vacua.longitudo()
  nota vacua[0]
  nota vacua?.longitudo()
}
",
    );
    let mut vacua_nodes = Vec::new();
    walk_exprs(&program, &mut |expr| {
        if matches!(expr.kind, ExprKind::Vacua(_)) {
            vacua_nodes.push(expr.id);
        }
    });
    assert_eq!(
        vacua_nodes.len(),
        4,
        "expected marker claims in all four positions"
    );
    assert!(
        vacua_nodes
            .iter()
            .all(|node| resolver.shadowed_builtin(*node).is_some()),
        "every marker expression position must resolve the live binding"
    );
}

#[test]
fn initializer_self_reference_falls_back_to_marker_without_outer_binding() {
    let (program, resolver, _) = resolve_ok(
        r"
functio probe() → lista<numerus> {
  fixum lista<numerus> vacua ← vacua
  redde vacua
}
",
    );
    let mut vacua_nodes = Vec::new();
    walk_exprs(&program, &mut |expr| {
        if matches!(expr.kind, ExprKind::Vacua(_)) {
            vacua_nodes.push(expr.id);
        }
    });
    assert_eq!(
        vacua_nodes.len(),
        1,
        "expected one initializer marker claim"
    );
    assert!(
        resolver.shadowed_builtin(vacua_nodes[0]).is_none(),
        "a declaration is not live until its initializer completes"
    );
}

#[test]
fn initializer_self_reference_uses_live_outer_binding() {
    let (program, resolver, _) = resolve_ok(
        r"
functio probe() → lista<numerus> {
  fixum lista<numerus> vacua ← [1, 2, 3]
  {
    fixum lista<numerus> vacua ← vacua
    redde vacua
  }
}
",
    );
    let mut vacua_nodes = Vec::new();
    walk_exprs(&program, &mut |expr| {
        if matches!(expr.kind, ExprKind::Vacua(_)) {
            vacua_nodes.push(expr.id);
        }
    });
    assert_eq!(
        vacua_nodes.len(),
        1,
        "expected one outer-binding initializer claim"
    );
    assert!(
        resolver.shadowed_builtin(vacua_nodes[0]).is_some(),
        "an outer same-named binding must win in the initializer"
    );
}

#[test]
fn marker_resolution_reverts_after_scope_exit() {
    let (program, resolver, _) = resolve_ok(
        r"
incipit {
  {
    fixum lista<numerus> vacua ← [1, 2, 3]
    nota vacua
  }
  nota vacua
}
",
    );
    let mut vacua_nodes = Vec::new();
    walk_exprs(&program, &mut |expr| {
        if matches!(expr.kind, ExprKind::Vacua(_)) {
            vacua_nodes.push(expr.id);
        }
    });
    assert_eq!(
        vacua_nodes.len(),
        2,
        "expected inner binding and post-scope marker claims"
    );
    assert!(
        resolver.shadowed_builtin(vacua_nodes[0]).is_some(),
        "the inner reference must use the inner binding"
    );
    assert!(
        resolver.shadowed_builtin(vacua_nodes[1]).is_none(),
        "scope exit must restore the unbound marker fallback"
    );
}

#[test]
fn set_typed_marker_binding_is_a_list_argument_type_error() {
    test_support::assert_rejects_with_issue(
        r"
functio probe(lista<numerus> xs) { }
incipit {
  fixum copia<numerus> vacua ← [1, 2] ↦ copia<numerus>
  probe(vacua)
}
",
        "argument_type_mismatch",
    );
}

#[test]
fn en_read_binding_wins_over_lege_builtin() {
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path)
        .expect("installed en pack should load");
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(pack);
    let result = crate::test_support::analyze_source_with_config(
        r#"
main {
  const string read ← "data"
  print read
}
"#,
        config,
    );
    assert!(
        result.is_ok(),
        "en read binding must compile: {:?}",
        result.diagnostics
    );
    assert!(
        !has_issue(&result, "WARN001", "unused_variable"),
        "en read binding must be used: {:?}",
        result.diagnostics
    );
}

#[test]
fn en_read_fn_is_callable() {
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path)
        .expect("installed en pack should load");
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(pack);
    let result = crate::test_support::analyze_source_with_config(
        r#"
fn read(string file) → string {
  return file
}
main {
  const string x ← read("cfg")
  print x
}
"#,
        config,
    );
    assert!(
        result.is_ok(),
        "en read fn must be callable: {:?}",
        result.diagnostics
    );
    assert!(
        !has_issue(&result, "SEM012", "callee_not_callable"),
        "read(\"cfg\") must call the user fn: {:?}",
        result.diagnostics
    );
}
