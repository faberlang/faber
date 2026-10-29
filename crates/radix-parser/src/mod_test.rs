//! Parser behavior and structured diagnostic contract tests.
//!
//! Rendered diagnostic prose belongs in renderer/reader-pack contract tests,
//! not here. Shared parse plumbing lives in `parser::test_support`.

use crate::test_support::{
    ParseExpectation, assert_parse_error_has_no_legacy_message, assert_parse_error_issue,
    assert_parse_error_matches, has_arg, parse_ok, parse_program, span_for, symbol_name,
    test_span_between,
};
use crate::{ParseError, ParseErrorKind, ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{
    AnnotationKind, AstMutability, AstParamMode, AwaitBindingForm, BindingPattern, CallablePosture,
    ClassMemberKind, ClausuraBody, ExprKind, IfBody, ImportKind, IteraMode, JsonValue, Literal,
    Pattern, PatternBind, PraeparaKind, ProbaModifier, ScribeKind, SecusClause, StmtKind,
    TypeExprKind,
};

fn generic_arg_type(arg: &radix_syntax::GenericArgExpr) -> &radix_syntax::TypeExpr {
    match arg {
        radix_syntax::GenericArgExpr::Type(ty) => ty,
        other => panic!("expected type generic argument, got {other:?}"),
    }
}

#[test]
fn parses_capta_trap_expression() {
    // UT: `capta { ... }` parses in expression position; the block body is an
    // ordinary statement block whose trailing expression is the success value.
    let result = parse_ok(
        "functio f() → numerus ⇥ textus { redde 1 }\nfunctio g() → vacuum { sit ∪ r ← capta { f() } nota r }\n",
    );
    assert!(
        result.errors.is_empty(),
        "trap source must parse cleanly: {:?}",
        result.errors
    );
    let program = result.program.as_ref().expect("program");
    assert!(
        program.statements.len() == 2,
        "both functions parse: {:?}",
        program.statements.len()
    );
}

#[test]
fn capta_stays_usable_as_ordinary_identifier() {
    // The keyword is contextual (expression-primary position only): a
    // binding named `capta` and a call `capta()` stay ordinary identifiers.
    let result = parse_ok(
        "functio capta(numerus x) → numerus { redde x }\nfunctio g() → vacuum { sit numerus capta ← capta(1) nota capta }\n",
    );
    assert!(
        result.errors.is_empty(),
        "capta as identifier must parse: {:?}",
        result.errors
    );
}

#[test]
fn parses_empty_source() {
    let result = parse_ok("");
    let program = result.program.as_ref().expect("program");
    assert!(program.statements.is_empty());
}

#[test]
fn missing_function_name_emits_structured_parse_error() {
    let result = parse_program("functio () → vacuum {}\n");

    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::FunctionNameAbsent)
        .expect("missing function name diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    // The keyword spelling comes from the reader pack, not a literal arg
    //: a literal arg would beat the pack binding.
    assert!(err.args.is_empty());
}

#[test]
fn missing_class_name_emits_structured_parse_error() {
    let result = parse_program("genus {}\n");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::ClassNameAbsent)
        .expect("missing class name diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    // The keyword spelling comes from the reader pack, not a literal arg
    //: a literal arg would beat the pack binding.
    assert!(err.args.is_empty());
}

#[test]
fn missing_class_body_emits_structured_parse_error() {
    let result = parse_program("genus Nomen\n");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::ClassBodyAbsent)
        .expect("missing class body diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    // The keyword spelling comes from the reader pack, not a literal arg
    //: a literal arg would beat the pack binding.
    assert!(err.args.is_empty());
}

#[test]
fn bare_cli_optio_requires_string_default() {
    let result = parse_program(
        r"
@ cli
incipit argumenta args {}
",
    );
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::Expected
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "expected_string_literal")
        })
        .expect("structured expected diagnostic");

    assert_parse_error_has_no_legacy_message(err);
}

#[test]
fn bare_optio_annotation_expected_identifier() {
    let result = parse_program(
        r"
@ optio
",
    );
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::Expected
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "expected_identifier")
        })
        .expect("structured expected diagnostic");

    assert_parse_error_has_no_legacy_message(err);
}

#[test]
fn incipit_tacet_expected_brace() {
    let result = parse_program("incipit tacet");
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::Expected
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "block_start")
        })
        .expect("structured parser-core expected diagnostic");

    assert_parse_error_has_no_legacy_message(err);
}

#[test]
fn functio_missing_close_paren_expected() {
    let result = parse_program("functio f(textus name → vacuum {}");
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::Expected
                && err.args.iter().any(|arg| {
                    arg.name == "issue" && arg.value == "function_declaration_parameters_end"
                })
        })
        .expect("structured parser-core expected diagnostic");

    assert_parse_error_has_no_legacy_message(err);
}

#[test]
fn futura_annotation_precedes_functio_declaration_on_next_line() {
    let result = parse_ok("@ futura\nfunctio f() → vacuum { }");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Func(_) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    assert_eq!(program.statements[0].annotations.len(), 1);

    let AnnotationKind::Statement(stmt) = &program.statements[0].annotations[0].kind else {
        panic!("expected statement annotation for @ futura");
    };
    assert_eq!(symbol_name(&result, stmt.name.name), "futura");
    assert!(
        stmt.args.is_empty(),
        "functio must not be consumed as annotation arg: {:?}",
        stmt.args
    );
}

#[test]
fn publica_annotation_stops_before_type_alias_declaration() {
    // Type aliases are declaration starts after a visibility annotation; the
    // alias declaration must not be swallowed as annotation args (MB-U1 / tela
    // U5: `@ publica type Nomen = string` joins the import surface).
    let result = parse_ok("@ publica\ntypus Nomen = string\n");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1, "type alias is a statement");

    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias declaration");
    };
    assert_eq!(symbol_name(&result, alias.name.name), "Nomen");
    assert_eq!(program.statements[0].annotations.len(), 1);
    assert!(matches!(
        program.statements[0].annotations[0].kind,
        AnnotationKind::Publica
    ));
}

#[test]
fn publica_annotation_rejects_same_line_type_alias() {
    // A bare annotation is NEWLINE-terminated ([043]); the alias declaration
    // it annotates must start on the next physical line, not share it.
    assert_parse_error_matches(
        "@ publica typus Nomen = string\n",
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ publica"),
        ]),
    );
}

#[test]
fn parses_braced_futura_cli_and_optio_annotations() {
    let result = parse_ok(
        r#"
@ futura { }
@ cli { nomen = "smoke" }
@ optio {
    binding = verbose,
    brevis = "v",
    longum = "verbose",
    typus = bivalens,
    descriptio = "Verbose mode"
}
functio f() → vacuum { }
"#,
    );

    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    assert_eq!(program.statements[0].annotations.len(), 3);

    let AnnotationKind::Braced(futura) = &program.statements[0].annotations[0].kind else {
        panic!("expected braced @ futura");
    };
    assert_eq!(symbol_name(&result, futura.name.name), "futura");
    assert!(futura.fields.is_empty());

    let AnnotationKind::Braced(cli) = &program.statements[0].annotations[1].kind else {
        panic!("expected braced @ cli");
    };
    assert_eq!(symbol_name(&result, cli.name.name), "cli");
    assert_eq!(cli.fields.len(), 1);
    assert_eq!(symbol_name(&result, cli.fields[0].name.name), "nomen");

    let AnnotationKind::Braced(optio) = &program.statements[0].annotations[2].kind else {
        panic!("expected braced @ optio");
    };
    assert_eq!(symbol_name(&result, optio.name.name), "optio");
    assert_eq!(optio.fields.len(), 5);
}

#[test]
fn parses_interna_annotation_on_function() {
    let result = parse_ok("@ interna\nfunctio hidden() → vacuum {\n}\n");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    assert_eq!(program.statements[0].annotations.len(), 1);
    assert!(matches!(
        program.statements[0].annotations[0].kind,
        AnnotationKind::Interna
    ));
}

#[test]
fn parses_interna_annotation_on_class_member() {
    let result =
        parse_ok("genus Demo {\n    @ interna\n    functio hidden() → vacuum {\n    }\n}\n");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected class declaration");
    };
    assert_eq!(class.members.len(), 1);
    assert_eq!(class.members[0].annotations.len(), 1);
    assert!(matches!(
        class.members[0].annotations[0].kind,
        AnnotationKind::Interna
    ));
}

#[test]
fn stacked_braced_annotations_on_one_line_before_functio() {
    let result = parse_ok(
        "@ futura { } @ optio { binding = verbose, brevis = \"v\" } functio f() → vacuum { }",
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    assert_eq!(program.statements[0].annotations.len(), 2);
    assert!(matches!(
        program.statements[0].annotations[0].kind,
        AnnotationKind::Braced(_)
    ));
    assert!(matches!(
        program.statements[0].annotations[1].kind,
        AnnotationKind::Braced(_)
    ));
    assert!(matches!(program.statements[0].kind, StmtKind::Func(_)));
}

#[test]
fn parses_compact_braced_futura_before_functio_without_whitespace() {
    let result = parse_ok("@futura{}functio f() → vacuum { }");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    assert_eq!(program.statements[0].annotations.len(), 1);
    let AnnotationKind::Braced(futura) = &program.statements[0].annotations[0].kind else {
        panic!("expected braced futura");
    };
    assert_eq!(symbol_name(&result, futura.name.name), "futura");
    assert!(matches!(program.statements[0].kind, StmtKind::Func(_)));
}

#[test]
fn parses_cli_and_imperium_annotations_as_structured_ast() {
    let result = parse_ok(
        r#"
@ cli "faber"
@ versio "0.1.0"
incipit argumenta args {}

@ imperium "emit"
@ alias "e"
functio emit() {}
"#,
    );

    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);

    assert!(matches!(
        &program.statements[0].annotations[0].kind,
        AnnotationKind::Cli(cli) if symbol_name(&result, cli.name) == "faber"
    ));
    assert!(
        matches!(
            program.statements[0].annotations[1].kind,
            AnnotationKind::Statement(_)
        ),
        "@ versio remains generic in phase 01"
    );

    assert!(matches!(
        &program.statements[1].annotations[0].kind,
        AnnotationKind::Imperium(command) if symbol_name(&result, command.name) == "emit"
    ));
    assert!(
        matches!(
            program.statements[1].annotations[1].kind,
            AnnotationKind::Statement(_)
        ),
        "@ alias remains generic in phase 01"
    );
}

#[test]
fn keeps_other_cli_metadata_annotations_generic() {
    let result = parse_ok(
        r#"
@ descriptio "Compile source files"
@ imperia "jobs" ex jobsModulum
incipit argumenta args {}
"#,
    );

    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements[0].annotations.len(), 2);

    let AnnotationKind::Statement(description) = &program.statements[0].annotations[0].kind else {
        panic!("@ descriptio should remain generic in phase 01");
    };
    assert_eq!(symbol_name(&result, description.name.name), "descriptio");
    assert_eq!(description.args.len(), 1);

    let AnnotationKind::Statement(imperia) = &program.statements[0].annotations[1].kind else {
        panic!("@ imperia should remain generic in phase 01");
    };
    assert_eq!(symbol_name(&result, imperia.name.name), "imperia");
    assert_eq!(imperia.args.len(), 3);
}

#[test]
fn parses_radix_typus_domain_annotation_as_structured_ast() {
    let result = parse_ok(
        r"
@ radix typus T in textus lista<textus> octeti
functio load<T>(textus via) → T {}
",
    );

    let program = result.program.as_ref().expect("program");
    let AnnotationKind::RadixTypusDomain(domain) = &program.statements[0].annotations[0].kind
    else {
        panic!("expected structured @ radix typus domain annotation");
    };
    assert_eq!(symbol_name(&result, domain.param.name), "T");
    assert_eq!(domain.allowed.len(), 3);
    assert!(matches!(
        &domain.allowed[0].kind,
        TypeExprKind::Named(name, params) if params.is_empty() && symbol_name(&result, name.name) == "textus"
    ));
    assert!(matches!(
        &domain.allowed[1].kind,
        TypeExprKind::Named(name, params) if params.len() == 1 && symbol_name(&result, name.name) == "lista"
    ));
    assert!(matches!(
        &domain.allowed[2].kind,
        TypeExprKind::Named(name, params) if params.is_empty() && symbol_name(&result, name.name) == "octeti"
    ));
}

#[test]
fn parses_repeated_radix_typus_domain_annotations() {
    let result = parse_ok(
        r"
@ radix typus T in i8 i16 i32 i64
@ radix typus T in u8 u16 u32 u64
@ radix typus T in f32 f64
functio numeric<T>(T value) → T {}
",
    );

    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements[0].annotations.len(), 3);
    for annotation in &program.statements[0].annotations {
        assert!(matches!(
            annotation.kind,
            AnnotationKind::RadixTypusDomain(_)
        ));
    }
}

#[test]
fn parses_radix_lane_annotation_as_structured_ast() {
    let result = parse_ok(
        r#"
@ radix lane "air"
functio compute() → vacuum {}
"#,
    );

    let program = result.program.as_ref().expect("program");
    let AnnotationKind::RadixLane(lane) = &program.statements[0].annotations[0].kind else {
        panic!("expected structured @ radix lane annotation");
    };
    assert_eq!(symbol_name(&result, lane.lane), "air");
}

#[test]
fn parses_radix_lane_with_mir_annotation() {
    let result = parse_ok(
        r#"
@ radix lane "mir"
functio process() → vacuum {}
"#,
    );

    let program = result.program.as_ref().expect("program");
    let AnnotationKind::RadixLane(lane) = &program.statements[0].annotations[0].kind else {
        panic!("expected structured @ radix lane annotation");
    };
    assert_eq!(symbol_name(&result, lane.lane), "mir");
}

#[test]
fn parses_radix_lane_with_hir_direct_annotation() {
    let result = parse_ok(
        r#"
@ radix lane "hir-direct"
functio render() → vacuum {}
"#,
    );

    let program = result.program.as_ref().expect("program");
    let AnnotationKind::RadixLane(lane) = &program.statements[0].annotations[0].kind else {
        panic!("expected structured @ radix lane annotation");
    };
    assert_eq!(symbol_name(&result, lane.lane), "hir-direct");
}

#[test]
fn parses_canonical_optio_annotation_as_structured_ast() {
    let result = parse_ok(
        r#"
@ optio target brevis "t" longum "target" typus lista<textus> descriptio "Target language" ubique vel "rust"
incipit argumenta args {}
"#,
    );

    let program = result.program.as_ref().expect("program");
    let AnnotationKind::Optio(optio) = &program.statements[0].annotations[0].kind else {
        panic!("expected structured @ optio");
    };

    assert_eq!(symbol_name(&result, optio.binding.name), "target");
    assert_eq!(symbol_name(&result, optio.short.expect("short")), "t");
    assert_eq!(symbol_name(&result, optio.long.expect("long")), "target");
    assert_eq!(
        symbol_name(&result, optio.description.expect("description")),
        "Target language"
    );
    assert!(!optio.is_flag);
    assert!(optio.global);
    assert!(optio.default.is_some());

    let ty = optio.ty.as_ref().expect("explicit type");
    let TypeExprKind::Named(name, params) = &ty.kind else {
        panic!("expected named option type");
    };
    assert_eq!(symbol_name(&result, name.name), "lista");
    assert_eq!(params.len(), 1);
    let TypeExprKind::Named(param_name, param_params) = &generic_arg_type(&params[0]).kind else {
        panic!("expected named type parameter");
    };
    assert_eq!(symbol_name(&result, param_name.name), "textus");
    assert!(param_params.is_empty());
}

#[test]
fn parses_bivalens_optio_type_as_boolean_flag() {
    let result = parse_ok(
        r#"
@ optio verbose longum "verbose" typus bivalens
incipit argumenta args {}
"#,
    );

    let program = result.program.as_ref().expect("program");
    let AnnotationKind::Optio(optio) = &program.statements[0].annotations[0].kind else {
        panic!("expected structured @ optio");
    };

    assert_eq!(symbol_name(&result, optio.binding.name), "verbose");
    assert!(optio.is_flag, "typus bivalens marks a boolean flag");
    assert!(
        optio.default.is_none(),
        "absent boolean flag defaults are applied by CLI validation/lowering"
    );
}

#[test]
fn parses_numeric_optio_default() {
    let result = parse_ok(
        r#"
@ optio limit longum "limit" typus numerus vel 100
incipit argumenta args {}
"#,
    );
    let program = result.program.as_ref().expect("program");
    let AnnotationKind::Optio(limit) = &program.statements[0].annotations[0].kind else {
        panic!("expected structured numeric @ optio");
    };
    let Some(default) = &limit.default else {
        panic!("expected numeric default");
    };
    assert!(matches!(default.kind, ExprKind::Literal(Literal::Int(100))));
}

#[test]
fn parses_bivalens_optio_default() {
    let result = parse_ok(
        r#"
@ optio strict longum "strict" typus bivalens vel verum
incipit argumenta args {}
"#,
    );
    let program = result.program.as_ref().expect("program");
    let AnnotationKind::Optio(strict) = &program.statements[0].annotations[0].kind else {
        panic!("expected structured boolean @ optio");
    };
    let Some(default) = &strict.default else {
        panic!("expected boolean default");
    };
    assert!(strict.is_flag);
    assert!(matches!(
        default.kind,
        ExprKind::Literal(Literal::Bool(true))
    ));
}

#[test]
fn omitted_optio_type_is_left_for_textus_defaulting() {
    let result = parse_ok(
        r#"
@ optio output longum "output" descriptio "Output path"
incipit argumenta args {}
"#,
    );

    let program = result.program.as_ref().expect("program");
    let AnnotationKind::Optio(optio) = &program.statements[0].annotations[0].kind else {
        panic!("expected structured @ optio");
    };

    assert_eq!(symbol_name(&result, optio.binding.name), "output");
    assert!(
        optio.ty.is_none(),
        "omitted typus means textus in later CLI validation/lowering"
    );
}

#[test]
fn parses_operandus_annotation_with_rest_type_global_and_default() {
    let result = parse_ok(
        r#"
@ operandus ceteri lista<textus> files descriptio "Input files" ubique vel "src"
incipit argumenta args {}
"#,
    );

    let program = result.program.as_ref().expect("program");
    let AnnotationKind::Operandus(operandus) = &program.statements[0].annotations[0].kind else {
        panic!("expected structured @ operandus");
    };

    assert!(operandus.is_rest);
    assert_eq!(symbol_name(&result, operandus.binding.name), "files");
    assert_eq!(
        symbol_name(&result, operandus.description.expect("description")),
        "Input files"
    );
    assert!(operandus.global);
    assert!(operandus.default.is_some());

    let TypeExprKind::Named(name, params) = &operandus.ty.kind else {
        panic!("expected named operand type");
    };
    assert_eq!(symbol_name(&result, name.name), "lista");
    assert_eq!(params.len(), 1);
}

#[test]
fn optio_historical_type_first_rejected() {
    assert_parse_error_matches(
        r#"
@ optio textus output longum "output"
incipit argumenta args {}
"#,
        ParseExpectation::Args(&[("issue", "invalid_modifier"), ("annotation", "optio")]),
    );
}

#[test]
fn optio_bare_bivalens_form_rejected() {
    assert_parse_error_matches(
        r#"
@ optio verbose longum "verbose" bivalens
incipit argumenta args {}
"#,
        ParseExpectation::Args(&[("issue", "invalid_modifier"), ("annotation", "optio")]),
    );
}

#[test]
fn optio_ignotum_modifier_rejected() {
    assert_parse_error_matches(
        r"
@ optio output ignotum
incipit argumenta args {}
",
        ParseExpectation::Args(&[("issue", "invalid_modifier"), ("annotation", "optio")]),
    );
}

#[test]
fn operandus_longum_modifier_in_annotation_error() {
    assert_parse_error_matches(
        r#"
@ operandus textus input longum "input"
incipit argumenta args {}
"#,
        ParseExpectation::Args(&[("issue", "invalid_modifier"), ("annotation", "operandus")]),
    );
}

#[test]
fn radix_typus_empty_domain_as_invalid_annotation() {
    assert_parse_error_matches(
        r"
@ radix typus T in
functio load<T>() → T {}
",
        ParseExpectation::Args(&[("issue", "empty_radix_typus_domain")]),
    );
}

#[test]
fn cli_extra_token_as_invalid_annotation() {
    assert_parse_error_matches(
        r#"
@ cli "faber" extra
incipit argumenta args {}
"#,
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ cli name"),
        ]),
    );
}

#[test]
fn secus_si_else_if_rejected() {
    assert_parse_error_matches(
        "si verum { tacet } secus si falsum { tacet }",
        ParseExpectation::Args(&[("issue", "secus_si_else_if")]),
    );
}

#[test]
fn ex_without_mutability_marker_rejected() {
    assert_parse_error_matches(
        r"ex data name",
        ParseExpectation::Args(&[
            ("issue", "mutability_marker"),
            ("expected", "'sit' or 'varia'"),
        ]),
    );
}

#[test]
fn itera_pro_mode_rejected() {
    assert_parse_error_matches(
        r"itera pro 0‥3 fixum i { nota i }",
        ParseExpectation::Args(&[("issue", "itera_mode"), ("expected", "'ex', 'de', or 'ab'")]),
    );
}

#[test]
fn parses_itera_loop_apud_clause_and_preserves_coordinate_order() {
    // Post-delivery admit shapes: tensor rank 1/2/3 with [i]/[i, j]/[i, j, k]
    // and matrix rank 2 with [i, j]. Arity-error shapes remain [i] on rank 2,
    // [i, j] on rank 3, and [i, j, k] on matrix rank 2.
    for (source, expected) in [
        (r"itera ex m apud [i] sit cell { }", &["i"][..]),
        (r"itera ex m apud [i, j] sit cell { }", &["i", "j"][..]),
        (
            r"itera ex m apud [i, j, k] sit cell { }",
            &["i", "j", "k"][..],
        ),
    ] {
        let result = parse_ok(source);
        let program = result.program.as_ref().expect("program");
        let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
            panic!("expected itera statement");
        };
        let coordinates = iter_stmt
            .apud
            .iter()
            .map(|ident| symbol_name(&result, ident.name))
            .collect::<Vec<_>>();
        assert_eq!(coordinates, expected);
        assert!(matches!(
            &iter_stmt.bindings[..],
            [BindingPattern::Ident(_)]
        ));
    }
}

#[test]
fn rejects_malformed_itera_apud_clauses_with_structured_errors() {
    for (source, arg, value) in [
        (
            r"itera ex m apud sit cell { }",
            "issue",
            "apud_coordinates_start",
        ),
        (
            r"itera ex m apud (i, j) sit cell { }",
            "issue",
            "apud_coordinates_start",
        ),
        (
            r"itera ex m apud [] sit cell { }",
            "issue",
            "expected_identifier",
        ),
        (
            r"itera ex m apud [i,] sit cell { }",
            "issue",
            "expected_identifier",
        ),
    ] {
        let result = parse_program(source);
        assert!(
            result.errors.iter().any(|error| {
                error.kind == ParseErrorKind::Expected && has_arg(error, arg, value)
            }),
            "expected structured apud rejection for {source:?}, got {:?}",
            result.errors
        );
    }
}

#[test]
fn sit_destructuring_parses_as_an_immutable_pattern_binding() {
    for source in [r"sit [a, b] ← [1, 2]", r"sit { a, b } ← point"] {
        let result = parse_ok(source);
        let program = result.program.as_ref().expect("program");
        let StmtKind::Var(decl) = &program.statements[0].kind else {
            panic!("expected a binding declaration for {source}");
        };
        assert_eq!(decl.mutability, AstMutability::Immutable, "{source}");
        assert!(
            matches!(
                decl.binding,
                BindingPattern::Array { .. } | BindingPattern::Object { .. }
            ),
            "{source}"
        );
        assert!(!decl.compile_time, "{source}");
    }
}

#[test]
fn duplicate_rest_pattern_rejected() {
    assert_parse_error_matches(
        r"sit [a, ceteri rest, ceteri again] ← values",
        ParseExpectation::Args(&[("issue", "duplicate_rest_pattern")]),
    );
}

#[test]
fn enum_member_negative_missing_number_rejected() {
    assert_parse_error_matches(
        r"ordo Color { Red = - }",
        ParseExpectation::Arg {
            name: "expected",
            value: "number",
        },
    );
}

#[test]
fn enum_member_missing_value_rejected() {
    assert_parse_error_matches(
        r"ordo Color { Red = }",
        ParseExpectation::Arg {
            name: "expected",
            value: "value",
        },
    );
}

#[test]
fn proba_temporis_modifier_requires_integer() {
    assert_parse_error_matches(
        r#"probandum "suite" { proba "case" temporis { adfirma verum } }"#,
        ParseExpectation::Args(&[("issue", "test_modifier_integer"), ("modifier", "temporis")]),
    );
}

#[test]
fn bare_at_annotation_name_rejected() {
    assert_parse_error_matches(
        "@\n",
        ParseExpectation::Args(&[("issue", "annotation_name")]),
    );
}

#[test]
fn bare_cli_annotation_requires_string_default() {
    assert_parse_error_matches(
        r"
@ cli
incipit argumenta args {}
",
        ParseExpectation::Issue("expected_string_literal"),
    );
}

#[test]
fn cli_annotation_extra_token_rejected() {
    assert_parse_error_matches(
        r#"
@ cli "faber" extra
incipit argumenta args {}
"#,
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ cli name"),
        ]),
    );
}

#[test]
fn bare_imperium_annotation_requires_string_default() {
    assert_parse_error_matches(
        r"
@ imperium
functio run() {}
",
        ParseExpectation::Issue("expected_string_literal"),
    );
}

#[test]
fn imperium_annotation_extra_token_rejected() {
    assert_parse_error_matches(
        r#"
@ imperium "run" extra
functio run() {}
"#,
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ imperium name"),
        ]),
    );
}

#[test]
fn radix_typus_domain_missing_in_clause_rejected() {
    assert_parse_error_matches(
        r"
@ radix typus T
functio load<T>() → T {}
",
        ParseExpectation::Args(&[("issue", "expected_annotation_ident"), ("expected", "in")]),
    );
}

#[test]
fn radix_typus_domain_empty_list_rejected() {
    assert_parse_error_matches(
        r"
@ radix typus T in
functio load<T>() → T {}
",
        ParseExpectation::Issue("empty_radix_typus_domain"),
    );
}

#[test]
fn radix_typus_domain_comma_separated_rejected() {
    assert_parse_error_matches(
        r"
@ radix typus T in textus, octeti
functio load<T>() → T {}
",
        ParseExpectation::Args(&[
            ("issue", "unexpected_annotation_token"),
            ("context", "@ radix typus domain"),
        ]),
    );
}

#[test]
fn bare_optio_annotation_requires_identifier() {
    assert_parse_error_matches(
        r"
@ optio
",
        ParseExpectation::Issue("expected_identifier"),
    );
}

#[test]
fn optio_brevis_requires_string_literal() {
    assert_parse_error_matches(
        r"
@ optio output brevis
incipit argumenta args {}
",
        ParseExpectation::Issue("expected_string_literal"),
    );
}

#[test]
fn optio_longum_requires_string_literal() {
    assert_parse_error_matches(
        r"
@ optio output longum
incipit argumenta args {}
",
        ParseExpectation::Issue("expected_string_literal"),
    );
}

#[test]
fn optio_descriptio_requires_string_literal() {
    assert_parse_error_matches(
        r"
@ optio output descriptio
incipit argumenta args {}
",
        ParseExpectation::Issue("expected_string_literal"),
    );
}

#[test]
fn optio_typus_requires_identifier() {
    assert_parse_error_matches(
        r"
@ optio output typus
{}
",
        ParseExpectation::Issue("expected_identifier"),
    );
}

#[test]
fn optio_vel_requires_expression() {
    assert_parse_error_matches(
        r"
@ optio output vel
{}
",
        ParseExpectation::Issue("expected_expression"),
    );
}

#[test]
fn operandus_requires_identifier_after_type() {
    assert_parse_error_matches(
        r"
@ operandus textus
{}
",
        ParseExpectation::Issue("expected_identifier"),
    );
}

#[test]
fn operandus_descriptio_requires_string_literal() {
    assert_parse_error_matches(
        r"
@ operandus textus input descriptio
incipit argumenta args {}
",
        ParseExpectation::Issue("expected_string_literal"),
    );
}

#[test]
fn operandus_longum_modifier_rejected() {
    assert_parse_error_matches(
        r#"
@ operandus textus input longum "input"
incipit argumenta args {}
"#,
        ParseExpectation::Args(&[("issue", "invalid_modifier"), ("annotation", "operandus")]),
    );
}

#[test]
fn operandus_vel_requires_expression() {
    assert_parse_error_matches(
        r"
@ operandus textus input vel
{}
",
        ParseExpectation::Issue("expected_expression"),
    );
}

#[test]
fn recovery_stops_at_transfer_statement_boundaries() {
    let result = parse_program(
        r"
fixum = 1
rumpe
",
    );

    assert_eq!(result.errors.len(), 1, "expected one parse error");

    let program = result
        .program
        .expect("parser should still produce a program");
    assert_eq!(
        program.statements.len(),
        1,
        "recovery should preserve the following statement"
    );
    assert!(matches!(program.statements[0].kind, StmtKind::Rumpe(_)));
}

#[test]
fn recovery_stops_at_block_closure() {
    let result = parse_program(
        r"
incipit {
    fixum = 1
    rumpe
}
",
    );

    assert_eq!(result.errors.len(), 1, "expected one parse error");

    let program = result
        .program
        .expect("parser should still produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Incipit(entry) = &program.statements[0].kind else {
        panic!("expected incipit statement");
    };

    let radix_syntax::IfBody::Block(block) = &entry.body else {
        panic!("expected block body");
    };

    assert_eq!(
        block.statements.len(),
        1,
        "recovery should preserve valid statements inside the block"
    );
    assert!(matches!(block.statements[0].kind, StmtKind::Rumpe(_)));
}

#[test]
fn recovery_consumes_modulus_when_it_is_not_a_statement_start() {
    let result = parse_program(
        r#"
incipit {
    nota { "a": 1 ∷ modulus<u32> }
}
"#,
    );

    assert!(
        !result.errors.is_empty(),
        "malformed modulus input must report a parse error"
    );
    assert!(
        result.program.is_some(),
        "parser recovery must still produce a program"
    );
}

#[test]
fn recovery_preserves_annotated_following_statements() {
    let result = parse_program(
        r"
fixum = 1
@ futura
functio vale() {}
",
    );

    assert_eq!(result.errors.len(), 1, "expected one parse error");

    let program = result
        .program
        .expect("parser should still produce a program");
    assert_eq!(
        program.statements.len(),
        1,
        "recovery should preserve the following declaration"
    );

    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };

    assert_eq!(
        program.statements[0].annotations.len(),
        1,
        "annotation should remain attached after recovery"
    );
    assert!(func.body.is_some());
}

#[test]
fn parses_following_variable_after_if_body_error() {
    let result = parse_program(
        r"
si verum
sit numerus postea ← 2
",
    );

    assert_eq!(result.errors.len(), 1, "expected one parse error");

    let program = result
        .program
        .expect("parser should still produce a program");
    assert_eq!(
        program.statements.len(),
        1,
        "recovery should preserve the following variable declaration"
    );

    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };

    let BindingPattern::Ident(name) = &var.binding else {
        panic!("expected identifier binding");
    };

    assert_eq!(result.interner.resolve(name.name), "postea");
}

#[test]
fn genus_field_defaults_with_literals_do_not_consume_following_fields() {
    let result = parse_ok(
        r"
genus Thing {
    varia bivalens visible = falsum
    varia bivalens enabled = verum
    varia textus maybe = nulla
    varia textus name
}
",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus");
    };
    assert_eq!(class.members.len(), 4);
    for member in &class.members[..3] {
        let ClassMemberKind::Field(field) = &member.kind else {
            panic!("expected field");
        };
        assert!(field.init.is_some(), "expected literal field default");
    }
}

#[test]
fn parses_import_with_public_alias() {
    let result = parse_ok(r#"importa ex "mod" publica Value ut Alias"#);
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };
    assert_eq!(symbol_name(&result, import.path), "mod");
    match &import.kind {
        ImportKind::Named { name, alias, .. } => {
            assert_eq!(symbol_name(&result, name.name), "Value");
            assert_eq!(
                symbol_name(&result, alias.as_ref().expect("alias").name),
                "Alias"
            );
        }
        ImportKind::Wildcard { .. } | ImportKind::Selective { .. } => {
            panic!("expected named import")
        }
    }
}

#[test]
fn import_with_empty_path_rejected() {
    assert_parse_error_matches(
        r#"importa ex "" Value"#,
        ParseExpectation::Kind(ParseErrorKind::ImportInvalid),
    );
}

#[test]
fn import_without_marker_binds_explicit_name() {
    let result = parse_ok(r#"importa ex "norma:solum" solum"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import");
    };
    let ImportKind::Named { inferred, name, .. } = &import.kind else {
        panic!("expected named import");
    };
    assert!(!inferred, "explicit name must bind, not infer");
    assert_eq!(symbol_name(&result, name.name), "solum");
}

#[test]
fn import_after_expression_start_infers_binding() {
    // The identifier starts a following expression statement, so the import
    // binding is inferred from the path and the statement is preserved.
    let result = parse_ok("importa ex \"norma:chorda\"\nchorda.retorta(\"abc\")\n");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2, "import + expression statement");
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import");
    };
    let ImportKind::Named { inferred, .. } = &import.kind else {
        panic!("expected named import");
    };
    assert!(inferred, "binding must be inferred");
}

#[test]
fn import_privata_marker_rejected() {
    assert_parse_error_matches(
        r#"importa ex "norma:solum" privata solum"#,
        ParseExpectation::Kind(ParseErrorKind::ImportInvalid),
    );
    assert_parse_error_matches(
        r#"importa ex "norma:solum" privata solum"#,
        ParseExpectation::Issue("import_privata_removed"),
    );
    // The marker and re-export words belong to the reader pack's keyword
    // rows; the parser passes no `keyword` argument, which would override
    // the pack.
    let result = parse_program(r#"importa ex "norma:solum" privata solum"#);
    let error = result
        .errors
        .iter()
        .find(|error| has_arg(error, "issue", "import_privata_removed"))
        .expect("import_privata_removed error");
    assert!(
        error.args.iter().all(|arg| arg.name != "keyword"),
        "parser must not pass a keyword argument: {:?}",
        error.args
    );
}

#[test]
fn import_record_privata_visibility_rejected() {
    assert_parse_error_matches(
        r#"importa { ex = "norma:solum", visibilitas = privata, nomen = solum }"#,
        ParseExpectation::Kind(ParseErrorKind::ImportInvalid),
    );
    assert_parse_error_matches(
        r#"importa { ex = "norma:solum", visibilitas = privata, nomen = solum }"#,
        ParseExpectation::Issue("invalid_import_visibility"),
    );
}

#[test]
fn parses_fixum_typed_variable() {
    let result = parse_ok("sit numerus count ← 1");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Var(const_var) = &program.statements[0].kind else {
        panic!("expected const variable declaration");
    };
    assert!(matches!(const_var.binding, BindingPattern::Ident(_)));
}

#[test]
fn parses_varia_destructuring() {
    let result = parse_ok("varia [head, ceteri tail] ← values");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Var(array_var) = &program.statements[0].kind else {
        panic!("expected destructuring variable declaration");
    };
    let BindingPattern::Array {
        elements,
        is_rest: rest,
        ..
    } = &array_var.binding
    else {
        panic!("expected array binding pattern");
    };
    assert_eq!(elements.len(), 1);
    assert_eq!(
        symbol_name(&result, rest.as_ref().expect("rest binding").name),
        "tail"
    );
}

#[test]
fn parses_object_destructuring() {
    let result = parse_ok("sit { nomen ut n, aetas } ← persona");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Var(object_var) = &program.statements[0].kind else {
        panic!("expected object destructuring variable declaration");
    };
    let BindingPattern::Object {
        fields,
        is_rest: rest,
        ..
    } = &object_var.binding
    else {
        panic!("expected object binding pattern");
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(symbol_name(&result, fields[0].name.name), "nomen");
    assert_eq!(
        symbol_name(&result, fields[0].alias.as_ref().expect("alias").name),
        "n"
    );
    assert!(rest.is_none());
}

#[test]
fn parses_functio_with_all_modifiers() {
    let result = parse_ok(
        "@ futura\nfunctio mitte<T>(de textus nomen sponte ut alias vel \"anon\", own numerus item, ceteri T reliqua) errata Error exitus 7 immutata iacit optiones Opts → textus {}",
    );
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    assert_eq!(program.statements[0].annotations.len(), 1);
    assert!(matches!(
        program.statements[0].annotations[0].kind,
        AnnotationKind::Statement(_)
    ));
    assert_eq!(func.type_params.len(), 1);
    assert_eq!(func.params.len(), 3);
    assert!(func.body.is_some());
    assert!(func.params[0].sponte);
    assert!(matches!(func.params[0].mode, AstParamMode::Owned));
    assert_eq!(func.params[0].ty.mode, Some(radix_syntax::TypeMode::De));
    assert_eq!(
        symbol_name(&result, func.params[0].alias.as_ref().expect("alias").name),
        "alias"
    );
    assert!(func.params[0].default.is_some());
    assert!(matches!(func.params[1].mode, AstParamMode::Owned));
    assert_eq!(func.params[1].ty.mode, Some(radix_syntax::TypeMode::Own));
    assert!(func.params[2].is_rest);
    assert_eq!(func.modifiers.len(), 5);
}

#[test]
fn rejects_functio_without_body() {
    assert_parse_error_matches(
        "functio greet(textus name) → textus",
        ParseExpectation::Kind(ParseErrorKind::FunctionBodyAbsent),
    );
}

#[test]
fn parses_typus_alias() {
    let result = parse_ok("typus Nomen = textus");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias");
    };
    assert_eq!(symbol_name(&result, alias.name.name), "Nomen");
}

#[test]
fn parses_ordo_enum() {
    let result = parse_ok("ordo Status { Bonus = 1, Malus = -1, Textus = \"x\" }");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Enum(enum_decl) = &program.statements[0].kind else {
        panic!("expected enum declaration");
    };
    assert_eq!(enum_decl.members.len(), 3);
}

#[test]
fn parses_empty_ordo() {
    let result = parse_ok("ordo Empty {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Enum(enum_decl) = &program.statements[0].kind else {
        panic!("expected enum declaration");
    };
    assert!(enum_decl.members.is_empty());
}

#[test]
fn parses_discretio_union() {
    let result = parse_ok("discretio Resultatus<T> { Ok { T valor }, Err { textus nuntius } }");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Union(union_decl) = &program.statements[0].kind else {
        panic!("expected union declaration");
    };
    assert_eq!(union_decl.type_params.len(), 1);
    assert_eq!(union_decl.variants.len(), 2);
    assert_eq!(union_decl.variants[0].fields.len(), 1);
}

#[test]
fn parses_empty_discretio() {
    let result = parse_ok("discretio Empty {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Union(union_decl) = &program.statements[0].kind else {
        panic!("expected union declaration");
    };
    assert!(union_decl.variants.is_empty());
}

#[test]
fn parses_minimal_import_sugar_with_default_private_name() {
    let result = parse_ok(
        r#"
importa ex "norma:json/solve"
functio f() → vacuum { }
"#,
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);

    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };
    assert_eq!(symbol_name(&result, import.path), "norma:json/solve");
    assert_eq!(import.visibility, radix_syntax::Visibility::Privata);
    let ImportKind::Named {
        name,
        alias,
        inferred,
    } = &import.kind
    else {
        panic!("expected named import");
    };
    assert_eq!(symbol_name(&result, name.name), "solve");
    assert!(alias.is_none());
    assert!(*inferred);
}

#[test]
fn minimal_import_sugar_does_not_consume_following_statement_keyword() {
    let result = parse_ok(
        r#"
importa ex "norma:chorda"
nota "importa-minimal"
"#,
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);

    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };
    let ImportKind::Named { name, inferred, .. } = &import.kind else {
        panic!("expected named import");
    };
    assert_eq!(symbol_name(&result, name.name), "chorda");
    assert!(*inferred);
    assert!(matches!(program.statements[1].kind, StmtKind::Scribe(_)));
}

#[test]
fn parses_braced_import_defaults() {
    let result = parse_ok(r#"importa { ex = "norma:chorda" }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };

    assert_eq!(symbol_name(&result, import.path), "norma:chorda");
    assert_eq!(import.visibility, radix_syntax::Visibility::Privata);
    let ImportKind::Named {
        name,
        alias,
        inferred,
    } = &import.kind
    else {
        panic!("expected named import");
    };
    assert_eq!(symbol_name(&result, name.name), "chorda");
    assert!(alias.is_none());
    assert!(*inferred);
}

#[test]
fn parses_braced_import_alias_and_visibility() {
    let result =
        parse_ok(r#"importa { ex = "norma:json/solve", visibilitas = publica, ut = solve_mod }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };

    assert_eq!(import.visibility, radix_syntax::Visibility::Publica);
    let ImportKind::Named {
        name,
        alias,
        inferred,
    } = &import.kind
    else {
        panic!("expected named import");
    };
    assert_eq!(symbol_name(&result, name.name), "solve");
    assert_eq!(
        symbol_name(&result, alias.as_ref().expect("alias").name),
        "solve_mod"
    );
    assert!(*inferred);
}

#[test]
fn parses_braced_wildcard_import() {
    let result = parse_ok(r#"importa { ex = "lodash", omnia = lodash }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };

    let ImportKind::Wildcard { alias } = &import.kind else {
        panic!("expected wildcard import");
    };
    assert_eq!(symbol_name(&result, alias.name), "lodash");
}

#[test]
fn minimal_import_requires_inferrable_name() {
    let result = parse_program(r#"importa ex "faber:*""#);
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::ImportInvalid)
        .expect("invalid import diagnostic");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "import_name_inference_failed")
    );
}

#[test]
fn import_unknown_field_rejected() {
    assert_parse_error_matches(
        r#"importa { alienum = "norma:chorda" }"#,
        ParseExpectation::Issue("unknown_import_field"),
    );
}

fn last_span_for(source: &str, needle: &str) -> radix_lexer::Span {
    let start = u32::try_from(source.rfind(needle).expect("needle in source"))
        .expect("test fixture byte offset fits u32");
    test_span_between(
        start,
        start + u32::try_from(needle.len()).expect("test fixture byte length fits u32"),
    )
}

#[test]
fn import_unknown_field_error_spans_field_name() {
    let source = r#"importa { ex = "norma:chorda", alienum = 5, nomen = chorda }"#;
    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| has_arg(err, "issue", "unknown_import_field"))
        .expect("unknown_import_field diagnostic");
    assert_eq!(err.span, span_for(source, "alienum"));
}

#[test]
fn import_duplicate_ex_field_rejected_at_second_field() {
    let source = r#"importa { ex = "norma:chorda", nomen = chorda, ex = "norma:solum" }"#;
    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| has_arg(err, "issue", "duplicate_import_field"))
        .expect("duplicate_import_field diagnostic");
    assert!(has_arg(err, "field", "ex"));
    assert_eq!(err.span, last_span_for(source, "ex"));
}

#[test]
fn import_duplicate_nomen_field_rejected_at_second_field() {
    let source = r#"importa { ex = "norma:chorda", nomen = chorda, nomen = solum }"#;
    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| has_arg(err, "issue", "duplicate_import_field"))
        .expect("duplicate_import_field diagnostic");
    assert!(has_arg(err, "field", "nomen"));
    assert_eq!(err.span, last_span_for(source, "nomen"));
}

#[test]
fn import_missing_source_rejected() {
    assert_parse_error_matches(
        r"importa { nomen = chorda }",
        ParseExpectation::Issue("missing_import_source"),
    );
}

#[test]
fn import_mixed_wildcard_and_named_rejected() {
    assert_parse_error_matches(
        r#"importa { ex = "norma:chorda", nomen = chorda, omnia = all }"#,
        ParseExpectation::Issue("mixed_wildcard_and_named_import"),
    );
}

#[test]
fn import_invalid_visibility_rejected() {
    assert_parse_error_matches(
        r#"importa { ex = "norma:chorda", visibilitas = aliena }"#,
        ParseExpectation::Issue("invalid_import_visibility"),
    );
}

#[test]
fn import_name_inference_failed_rejected() {
    let result = parse_program(r#"importa ex "faber:*""#);
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::ImportInvalid)
        .expect("invalid import diagnostic");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "import_name_inference_failed")
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "path" && arg.value == "faber:*"),
        "import name inference diagnostic should expose path, got {:?}",
        err.args
    );
}

#[test]
fn parses_infer_marker_as_type_annotation() {
    let result = parse_ok(
        r#"
sit _ name ← "Marcus"
varia _ count ← 1
functio answer() → _ { redde 42 }
"#,
    );

    let program = result.program.as_ref().expect("program");

    let StmtKind::Var(name_var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    assert!(matches!(
        name_var.ty.as_ref().expect("explicit infer marker").kind,
        TypeExprKind::Infer
    ));

    let StmtKind::Func(func) = &program.statements[2].kind else {
        panic!("expected function declaration");
    };
    assert!(matches!(
        func.ret.as_ref().expect("explicit inferred return").kind,
        TypeExprKind::Infer
    ));
}

#[test]
fn parses_sit_as_inferred_immutable_binding() {
    let result = parse_ok(
        r#"
sit _ nomen ← "Marcus"
sit _ annus ← 42
"#,
    );

    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);

    for stmt in &program.statements {
        let StmtKind::Var(var) = &stmt.kind else {
            panic!("expected variable declaration");
        };
        assert_eq!(var.mutability, AstMutability::Immutable);
        assert!(matches!(
            var.ty.as_ref().expect("inferred type").kind,
            TypeExprKind::Infer
        ));
        assert!(var.init.is_some());
    }
}

#[test]
fn parses_sit_deferred_binding_without_initializer() {
    // TARGET: `sit _ x` (no initializer) is the inferred deferred immutable.
    // It parses with an inferred type slot and no init.
    let result = parse_ok("sit _ nomen");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    assert_eq!(var.mutability, AstMutability::Immutable);
    assert!(matches!(
        var.ty.as_ref().expect("inferred type").kind,
        TypeExprKind::Infer
    ));
    assert!(
        var.init.is_none(),
        "deferred sit binding should have no initializer"
    );
}

#[test]
fn parses_fixum_deferred_binding_without_initializer() {
    // TARGET: typed deferred immutable — `fixum numerus x` with no initializer.
    let result = parse_ok(
        r"
sit numerus pending
sit _ label
",
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);

    let StmtKind::Var(typed) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    assert_eq!(typed.mutability, AstMutability::Immutable);
    assert!(matches!(
        typed.ty.as_ref().expect("typed slot").kind,
        TypeExprKind::Named(_, _)
    ));
    assert!(
        typed.init.is_none(),
        "deferred fixum binding should have no initializer"
    );

    let StmtKind::Var(inferred) = &program.statements[1].kind else {
        panic!("expected variable declaration");
    };
    assert_eq!(inferred.mutability, AstMutability::Immutable);
    assert!(matches!(
        inferred.ty.as_ref().expect("inferred slot").kind,
        TypeExprKind::Infer
    ));
    assert!(inferred.init.is_none());
}

#[test]
fn sit_underscore_binding_is_the_inferred_slot() {
    let result = parse_ok(r"sit _ nomen ← 1");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    assert_eq!(var.mutability, AstMutability::Immutable);
    assert!(matches!(
        var.ty.as_ref().expect("inferred slot").kind,
        TypeExprKind::Infer
    ));
}

#[test]
fn sit_accepts_tokenless_keyword_as_binding_name() {
    // `varia` is tokenless since Batch E: `sit varia ← 1` binds a variable
    // named `varia`. Pre-migration the reserved Varia token made any binding
    // named `varia` a parse error (`sit_varia_modifier_rejected`).
    let result = parse_ok("sit _ varia ← 1");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let BindingPattern::Ident(name) = &var.binding else {
        panic!("expected identifier binding");
    };
    assert_eq!(symbol_name(&result, name.name), "varia");
}

#[test]
fn fixum_bare_name_rejected() {
    assert_parse_error_matches(
        r#"fixum name ← "Marcus""#,
        ParseExpectation::Issue("binding_type_slot_required"),
    );
}

#[test]
fn varia_bare_name_rejected() {
    assert_parse_error_matches(
        r"varia count ← 1",
        ParseExpectation::Issue("binding_type_slot_required"),
    );
}

#[test]
fn object_destructuring_rejects_colon_rename() {
    assert_parse_error_matches(
        r"sit { nomen: n } ← persona",
        ParseExpectation::Arg {
            name: "issue",
            value: "object_binding_pattern_end",
        },
    );
}

#[test]
fn parses_generic_genus_with_implements_and_members() {
    let result = parse_ok(
        r#"
genus Animal<T> implet Canens, Currens {
    @ futura
    functio canta(textus vox) → vacuum {}
    fixum textus nomen = "leo"
}
"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected class declaration");
    };
    assert_eq!(class.implements.len(), 2);
    assert_eq!(class.members.len(), 2);
    assert!(matches!(class.members[0].kind, ClassMemberKind::Method(_)));
    assert!(matches!(class.members[1].kind, ClassMemberKind::Field(_)));
    assert_eq!(class.members[0].annotations.len(), 1);

    let ClassMemberKind::Field(static_field) = &class.members[1].kind else {
        panic!("expected static field");
    };
    assert!(static_field.is_static());
}

#[test]
fn parses_implendum_interface_with_type_params() {
    let result = parse_ok(
        r"
implendum Canens<T> {
    functio canta(textus vox) iacit → vacuum
    functio aliter() → textus
}
",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Interface(interface) = &program.statements[0].kind else {
        panic!("expected interface declaration");
    };
    assert_eq!(interface.type_params.len(), 1);
    assert_eq!(interface.methods.len(), 2);
    assert_eq!(interface.methods[0].modifiers.len(), 1);
}

#[test]
fn parses_probandum_with_all_modifiers() {
    let result = parse_ok(
        r#"
probandum "suite" tag "parser" {
    praepara omnia {}
    postpara {}
    proba "case" erratur omitte "skip" futurum "later" solum tag "focus" temporis 5 metior repete 2 fragilis 1 solum_in "ci" {}
    probandum "nested" { proba "inner" {} }
}
"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Probandum(test) = &program.statements[0].kind else {
        panic!("expected probandum declaration");
    };
    assert_eq!(test.modifiers.len(), 1);
    assert!(matches!(test.modifiers[0], ProbaModifier::Tag(_)));
    assert_eq!(test.body.setup.len(), 2);
    assert!(matches!(test.body.setup[0].kind, PraeparaKind::Praepara));
    assert!(test.body.setup[0].all);
    assert!(matches!(test.body.setup[1].kind, PraeparaKind::Postpara));
    assert_eq!(test.body.tests.len(), 1);
    assert_eq!(test.body.nested.len(), 1);

    let case = &test.body.tests[0];
    assert_eq!(case.modifiers.len(), 10);
    assert!(matches!(case.modifiers[0], ProbaModifier::Erratur));
    assert!(matches!(case.modifiers[1], ProbaModifier::Omitte(_)));
    assert!(matches!(case.modifiers[2], ProbaModifier::Futurum(_)));
    assert!(matches!(case.modifiers[3], ProbaModifier::Solum));
    assert!(matches!(case.modifiers[4], ProbaModifier::Tag(_)));
    assert!(matches!(case.modifiers[5], ProbaModifier::Temporis(5)));
    assert!(matches!(case.modifiers[6], ProbaModifier::Metior));
    assert!(matches!(case.modifiers[7], ProbaModifier::Repete(2)));
    assert!(matches!(case.modifiers[8], ProbaModifier::Fragilis(1)));
    assert!(matches!(case.modifiers[9], ProbaModifier::SolumIn(_)));
}

#[test]
fn parses_structural_equals_fields_and_typed_construction() {
    let result = parse_ok(
        r#"
genus Point {
    varia numerus x = 0
    varia numerus y = 0
}

functio main() → numerus {
    sit _ p ← Point { x = 10, y = 20 }
    sit _ raw ← { "x": 10, "y": 20 }
    sit _ event ← finge Click { x = 1, y = 2 } ∷ Event
    redde p.x
}
"#,
    );

    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected class declaration");
    };
    let ClassMemberKind::Field(field) = &class.members[0].kind else {
        panic!("expected field declaration");
    };
    assert!(field.init.is_some());

    let StmtKind::Func(func) = &program.statements[1].kind else {
        panic!("expected function declaration");
    };
    let Some(body) = &func.body else {
        panic!("expected function body");
    };

    // Genus construction (Type { = }) stays Faber = grammar.
    let StmtKind::Var(typed_ctor) = &body.statements[0].kind else {
        panic!("expected typed constructor binding");
    };
    let Some(init) = typed_ctor.init.as_deref() else {
        panic!("expected initializer");
    };
    assert!(matches!(init.kind, ExprKind::Verte(_)));

    // Bare { } is now a JSON valor literal (Goal Stage 2 of inline-json-valor).
    // The anonymous Faber record { key = expr } is retired.
    let StmtKind::Var(raw_object) = &body.statements[1].kind else {
        panic!("expected raw object binding");
    };
    let Some(init) = raw_object.init.as_deref() else {
        panic!("expected initializer");
    };
    let ExprKind::Literal(Literal::JsonValor(JsonValue::Object(members))) = &init.kind else {
        panic!("expected JSON valor object literal, got {:?}", init.kind);
    };
    assert_eq!(members.len(), 2);
    assert!(matches!(members[0].value, JsonValue::Numerus(10)));
    assert!(matches!(members[1].value, JsonValue::Numerus(20)));

    // finge variant construction keeps Faber = grammar.
    let StmtKind::Var(finge_var) = &body.statements[2].kind else {
        panic!("expected finge binding");
    };
    let Some(init) = finge_var.init.as_deref() else {
        panic!("expected initializer");
    };
    let ExprKind::Finge(finge) = &init.kind else {
        panic!("expected finge expression");
    };
    assert_eq!(finge.fields.len(), 2);
}

#[test]
fn parses_applied_generic_typed_constructor_with_size_argument() {
    // `Gen<3> { lanes = … }` routes the applied head to the typed-constructor
    // branch, not comparison: the `verte` target carries the kinded size
    // argument on the existing applied `TypeExpr` form (S0-G1).
    let result = parse_ok(
        r"
genus Gen<magnitudo N> {
    varia vector<f32, magnitudo N> lanes
}

functio facere(vector<f32, magnitudo 3> lanes) → Gen<3> {
    redde Gen<3> { lanes = lanes }
}
",
    );

    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Func(func) = &program.statements[1].kind else {
        panic!("expected function declaration");
    };
    let Some(body) = &func.body else {
        panic!("expected function body");
    };
    let StmtKind::Redde(ret) = &body.statements[0].kind else {
        panic!(
            "expected return statement, got {:?}",
            body.statements[0].kind
        );
    };
    let ret_value = ret.value.as_deref().expect("return value");
    let ExprKind::Verte(verte) = &ret_value.kind else {
        panic!("expected verte constructor, got {:?}", ret_value.kind);
    };
    let TypeExprKind::Named(head, params) = &verte.ty.kind else {
        panic!("expected named applied head, got {:?}", verte.ty.kind);
    };
    assert_eq!(symbol_name(&result, head.name), "Gen");
    assert_eq!(params.len(), 1);
    assert!(matches!(params[0], radix_syntax::GenericArgExpr::Size(_)));
}

#[test]
fn parses_list_hole_bracket_constructor() {
    // `lista<_>[1, 2]` is a stated bracket constructor: the collection head
    // claims the `[ … ]` element list and the array becomes the `verte`
    // target, instead of `lista` falling back to an identifier and `<` to a
    // comparison.
    let result = parse_ok(
        r"
functio facere() → lista<numerus> {
    redde lista<_>[1, 2]
}
",
    );

    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    let Some(body) = &func.body else {
        panic!("expected function body");
    };
    let StmtKind::Redde(ret) = &body.statements[0].kind else {
        panic!(
            "expected return statement, got {:?}",
            body.statements[0].kind
        );
    };
    let ret_value = ret.value.as_deref().expect("return value");
    let ExprKind::Verte(verte) = &ret_value.kind else {
        panic!("expected verte constructor, got {:?}", ret_value.kind);
    };
    let ExprKind::Array(array) = &verte.expr.kind else {
        panic!("expected array target, got {:?}", verte.expr.kind);
    };
    assert_eq!(array.elements.len(), 2);
    let TypeExprKind::Named(head, params) = &verte.ty.kind else {
        panic!("expected named applied head, got {:?}", verte.ty.kind);
    };
    assert_eq!(symbol_name(&result, head.name), "lista");
    assert_eq!(params.len(), 1);

    // The empty-brace collection head stays unclaimed (no typed zero).
    assert_parse_error_issue(
        "functio facere() → lista<numerus> { redde lista<numerus> {} }",
        "constructor_type_not_claimed",
    );
}

#[test]
fn parses_list_bracket_constructor_in_nested_positions() {
    // The stated bracket constructor is a value expression at any depth: a
    // call argument, a parenthesized initializer, and the elements of another
    // list constructor all build the `verte` over an array.
    let result = parse_ok(
        r"
functio f(lista<numerus> xs) → vacuum {}

functio facere() → lista<lista<numerus>> {
    f(lista<numerus>[1, 2])
    sit _ y ← (lista<numerus>[3, 4])
    redde lista<lista<numerus>>[lista<numerus>[1], lista<numerus>[2]]
}
",
    );

    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Func(func) = &program.statements[1].kind else {
        panic!("expected function declaration");
    };
    let Some(body) = &func.body else {
        panic!("expected function body");
    };

    let StmtKind::Expr(call_stmt) = &body.statements[0].kind else {
        panic!(
            "expected expression statement, got {:?}",
            body.statements[0].kind
        );
    };
    let ExprKind::Call(call) = &call_stmt.expr.kind else {
        panic!("expected call, got {:?}", call_stmt.expr.kind);
    };
    let ExprKind::Verte(verte) = &call.args[0].value.kind else {
        panic!(
            "expected verte constructor argument, got {:?}",
            call.args[0].value.kind
        );
    };
    assert!(
        matches!(verte.expr.kind, ExprKind::Array(_)),
        "expected array target, got {:?}",
        verte.expr.kind
    );

    let StmtKind::Redde(ret) = &body.statements[2].kind else {
        panic!(
            "expected return statement, got {:?}",
            body.statements[2].kind
        );
    };
    let ret_value = ret.value.as_deref().expect("return value");
    let ExprKind::Verte(outer) = &ret_value.kind else {
        panic!("expected verte constructor, got {:?}", ret_value.kind);
    };
    let ExprKind::Array(outer_array) = &outer.expr.kind else {
        panic!("expected array target, got {:?}", outer.expr.kind);
    };
    assert_eq!(outer_array.elements.len(), 2);
}

#[test]
fn tensor_long_form_constructor_stays_store_position_only() {
    // Only the collection head's bracket constructor moves into nested
    // positions; the typed-zero heads keep the store-position refusal.
    assert_parse_error_issue(
        "functio f(tensor<f32, [2]> t) → vacuum {}\nincipit { f(tensor<f32, [2]> {}) }",
        "constructor_store_position_only",
    );
}

#[test]
fn parses_qualified_applied_generic_typed_constructor() {
    let result = parse_ok(
        r"
genus Gen<magnitudo N> {
    varia vector<f32, magnitudo N> lanes
}

functio facere(vector<f32, magnitudo 3> lanes) → Gen<3> {
    redde pkg.Gen<3> { lanes = lanes }
}
",
    );

    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Func(func) = &program.statements[1].kind else {
        panic!("expected function declaration");
    };
    let Some(body) = &func.body else {
        panic!("expected function body");
    };
    let StmtKind::Redde(ret) = &body.statements[0].kind else {
        panic!("expected return statement");
    };
    let ret_value = ret.value.as_deref().expect("return value");
    let ExprKind::Verte(verte) = &ret_value.kind else {
        panic!("expected verte constructor, got {:?}", ret_value.kind);
    };
    let TypeExprKind::Qualified(path, params) = &verte.ty.kind else {
        panic!("expected qualified applied head, got {:?}", verte.ty.kind);
    };
    assert_eq!(path.len(), 2);
    assert_eq!(params.len(), 1);
    assert!(matches!(params[0], radix_syntax::GenericArgExpr::Size(_)));
}

#[test]
fn applied_generic_head_without_field_block_stays_comparison() {
    // The speculative argument list must restore when no constructor field
    // block follows, so `a ≺ b` comparison parsing stays owned by the
    // binary operator branch (S0-G1 ambiguity guard).
    let result = parse_ok(
        r"
functio facere(numerus a, numerus b) → boolean {
    redde a ≺ b
}
",
    );

    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    let Some(body) = &func.body else {
        panic!("expected function body");
    };
    let StmtKind::Redde(ret) = &body.statements[0].kind else {
        panic!("expected return statement");
    };
    let ret_value = ret.value.as_deref().expect("return value");
    let ExprKind::Binary(bin) = &ret_value.kind else {
        panic!("expected binary comparison, got {:?}", ret_value.kind);
    };
    assert!(matches!(bin.op, radix_syntax::BinOp::Lt));
}

#[test]
fn applied_generic_head_without_field_block_stays_comparison_with_natural() {
    // `flag ≺ 3` — a natural literal argument alone must not claim the head;
    // only `< … > { fields }` does.
    let result = parse_ok(
        r"
functio facere(numerus flag) → boolean {
    redde flag ≺ 3
}
",
    );

    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    let Some(body) = &func.body else {
        panic!("expected function body");
    };
    let StmtKind::Redde(ret) = &body.statements[0].kind else {
        panic!("expected return statement");
    };
    let ret_value = ret.value.as_deref().expect("return value");
    let ExprKind::Binary(bin) = &ret_value.kind else {
        panic!("expected binary comparison, got {:?}", ret_value.kind);
    };
    assert!(matches!(bin.op, radix_syntax::BinOp::Lt));
}

#[test]
fn malformed_json_negative_number_reports_structured_unexpected_error() {
    let source = r#"sit _ value ← { "x": - }"#;
    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::Unexpected
                && err.args.iter().any(|arg| {
                    arg.name == "issue" && arg.value == "expected_json_number_after_minus"
                })
        })
        .expect("malformed JSON negative number diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 1);
    assert_eq!(err.args[0].name, "issue");
    assert_eq!(err.args[0].value, "expected_json_number_after_minus");
    assert_eq!(err.span, span_for(source, "}"));
}

#[test]
fn genus_field_colon_init_rejected() {
    assert_parse_error_matches(
        r"genus Point { numerus x : 0 }",
        ParseExpectation::Issue("expected_identifier"),
    );
}

#[test]
fn finge_colon_field_rejected() {
    assert_parse_error_matches(
        r"sit _ event ← finge Click { x: 1 } ∷ Event",
        ParseExpectation::Issue("field_value_separator"),
    );
}

#[test]
fn genus_colon_ctor_rejected() {
    assert_parse_error_matches(
        r"sit _ point ← Point { x: 10 }",
        ParseExpectation::Issue("expected_expression"),
    );
}

#[test]
fn bare_brace_equals_form_is_the_inferred_constructor() {
    // Operator ruling 2026-10-10: a bare `{ field = value }`
    // is sugar for `_ { field = value }`; the retired anonymous-record form
    // is gone, and the cell supplies the type (no cell is a checker error).
    let result = parse_ok(r"sit _ point ← { x = 10 }");
    assert!(result.errors.is_empty());
}

#[test]
fn bare_brace_colon_unquoted_key_rejected() {
    assert_parse_error_issue(r"sit _ point ← { x: 10 }", "json_quoted_key");
}

#[test]
fn inline_union_types_parse_flat() {
    let result = parse_ok("typus Maybe = textus ∪ numerus ∪ nihil");
    let program = result.program.as_ref().expect("program");
    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias");
    };
    let TypeExprKind::Union(members) = &alias.ty.kind else {
        panic!("expected flat union");
    };
    assert_eq!(members.len(), 3);
}

#[test]
fn proba_temporis_modifier_requires_number() {
    assert_parse_error_matches(
        r#"probandum "suite" { proba "case" temporis { adfirma verum } }"#,
        ParseExpectation::Args(&[("issue", "test_modifier_integer"), ("modifier", "temporis")]),
    );
}

#[test]
fn proba_repete_modifier_requires_number() {
    assert_parse_error_matches(
        r#"probandum "suite" { proba "case" repete { adfirma verum } }"#,
        ParseExpectation::Args(&[("issue", "test_modifier_integer"), ("modifier", "repete")]),
    );
}

#[test]
fn proba_fragilis_modifier_requires_number() {
    assert_parse_error_matches(
        r#"probandum "suite" { proba "case" fragilis { adfirma verum } }"#,
        ParseExpectation::Args(&[("issue", "test_modifier_integer"), ("modifier", "fragilis")]),
    );
}

#[test]
fn proba_modifiers_must_follow_name() {
    assert_parse_error_matches(
        r#"proba solum tag "parser" "parses input" {}"#,
        ParseExpectation::Issue("expected_string_literal"),
    );
}

#[test]
fn probandum_modifiers_must_follow_name() {
    assert_parse_error_matches(
        r#"probandum solum tag "parser" "suite" {}"#,
        ParseExpectation::Issue("expected_string_literal"),
    );
}

#[test]
fn parses_si_control_flow() {
    let result = parse_ok(
        r#"si verum ergo nota "yes" cape err {} sin falsum ergo redde 1 secus ergo tacet"#,
    );
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Si(if_stmt) = &program.statements[0].kind else {
        panic!("expected if statement");
    };
    assert!(matches!(if_stmt.then, IfBody::Ergo(_)));
    assert!(if_stmt.catch.is_some());
    let SecusClause::Sin { stmt: sin_stmt, .. } = if_stmt.else_.as_ref().expect("else clause")
    else {
        panic!("expected sin clause");
    };
    assert!(matches!(sin_stmt.then, IfBody::Ergo(_)));
    assert!(matches!(
        sin_stmt.else_.as_ref().expect("secus clause"),
        SecusClause::Stmt { .. }
    ));
}

#[test]
fn parses_dum_while_loop() {
    let result = parse_ok("dum verum { perge } cape err {}");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Dum(while_stmt) = &program.statements[0].kind else {
        panic!("expected while statement");
    };
    assert!(matches!(while_stmt.body, IfBody::Block(_)));
    assert!(while_stmt.catch.is_some());
}

#[test]
fn parses_itera_loop() {
    let result = parse_ok("itera ab items sit item ergo redde item cape err {}");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    assert!(matches!(iter_stmt.mode, IteraMode::Ab));
    assert!(matches!(iter_stmt.mutability, AstMutability::Immutable));
    assert!(matches!(iter_stmt.body, IfBody::Ergo(_)));
}

#[test]
fn parses_itera_array_pattern_binder() {
    let result = parse_ok("itera ex xs sit [a, b] { nota a }");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    assert!(matches!(iter_stmt.mode, IteraMode::Ex));
    let BindingPattern::Array {
        elements, is_rest, ..
    } = &iter_stmt.bindings[0]
    else {
        panic!("expected array binding pattern binder");
    };
    assert_eq!(elements.len(), 2);
    assert!(
        elements
            .iter()
            .all(|e| matches!(e, BindingPattern::Ident(_)))
    );
    assert!(is_rest.is_none());
}

#[test]
fn parses_itera_object_pattern_binder() {
    let result = parse_ok("itera ex xs sit {gx} { nota gx }");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    let BindingPattern::Object {
        fields, is_rest, ..
    } = &iter_stmt.bindings[0]
    else {
        panic!("expected object binding pattern binder");
    };
    assert_eq!(fields.len(), 1);
    assert!(is_rest.is_none());
}

#[test]
fn parses_itera_pattern_binder_all_modes() {
    for (source, mode) in [
        ("itera ex xs sit [a] { nota a }", IteraMode::Ex),
        ("itera de xs sit [a] { nota a }", IteraMode::De),
        ("itera ab items sit [a] { nota a }", IteraMode::Ab),
    ] {
        let result = parse_ok(source);
        let program = result
            .program
            .as_ref()
            .expect("parser should produce a program");
        let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
            panic!("expected itera statement for {source}");
        };
        // IteraMode has no PartialEq; discriminant equality is exact for this
        // fieldless enum and constrains the mode (unlike matches!(mode, mode)).
        assert_eq!(
            std::mem::discriminant(&iter_stmt.mode),
            std::mem::discriminant(&mode),
            "mode mismatch for {source}"
        );
        assert!(
            matches!(iter_stmt.bindings[0], BindingPattern::Array { .. }),
            "expected array binder for {source}"
        );
    }
}

#[test]
fn parses_itera_pattern_binder_element_forms() {
    let result = parse_ok("itera ex xs varia [_, secundus, ceteri cauda] { nota secundum }");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    assert!(matches!(iter_stmt.mutability, AstMutability::Mutable));
    let BindingPattern::Array {
        elements, is_rest, ..
    } = &iter_stmt.bindings[0]
    else {
        panic!("expected array binding pattern binder");
    };
    assert_eq!(elements.len(), 2);
    assert!(matches!(elements[0], BindingPattern::Wildcard(_)));
    assert!(matches!(elements[1], BindingPattern::Ident(_)));
    assert!(is_rest.is_some());
}

#[test]
fn parses_itera_object_pattern_alias() {
    let result = parse_ok("itera ex xs sit {gx ut caput} ergo redde caput cape err {}");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    let BindingPattern::Object { fields, .. } = &iter_stmt.bindings[0] else {
        panic!("expected object binding pattern binder");
    };
    assert_eq!(fields.len(), 1);
    assert!(fields[0].alias.is_some());
    assert!(iter_stmt.catch.is_some());
}

#[test]
fn parses_elige_switch() {
    let result = parse_ok("elige value { casu 1 ergo redde 1 ceterum ergo tacet } cape err {}");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Elige(switch_stmt) = &program.statements[0].kind else {
        panic!("expected elige statement");
    };
    assert_eq!(switch_stmt.cases.len(), 1);
    assert!(switch_stmt.default.is_some());
    assert!(switch_stmt.catch.is_some());
}

#[test]
fn parses_discerne_match() {
    let result = parse_ok(
        "discerne omnia value et other { casu Ok ut result et _ ergo redde result ceterum ergo mori \"bad\" }",
    );
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Discerne(match_stmt) = &program.statements[0].kind else {
        panic!("expected discerne statement");
    };
    assert!(match_stmt.exhaustive);
    assert_eq!(match_stmt.subjects.len(), 2);
    assert_eq!(match_stmt.arms.len(), 1);
    assert!(match_stmt.default.is_some());
    match &match_stmt.arms[0].patterns[0] {
        Pattern::Ident(_, Some(PatternBind::Alias(alias))) => {
            assert_eq!(symbol_name(&result, alias.name), "result");
        }
        _ => panic!("expected alias-bound pattern"),
    }
}

#[test]
fn parses_custodi_guard() {
    let result = parse_ok("custodi { si verum ergo redde 1 si falsum ergo nota \"no\" }");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Custodi(guard_stmt) = &program.statements[0].kind else {
        panic!("expected custodi statement");
    };
    assert_eq!(guard_stmt.clauses.len(), 2);
}

#[test]
fn parses_fac_do_while() {
    let result = parse_ok("fac {} cape err {} dum verum");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);

    let StmtKind::Fac(fac_stmt) = &program.statements[0].kind else {
        panic!("expected fac statement");
    };
    assert!(fac_stmt.catch.is_some());
    assert!(fac_stmt.while_.is_some());
}

/// Minimal en surface for guard-particle pins (`throw`/`panic`/`else`).
struct EnGuardPack;

impl KeywordPack for EnGuardPack {
    fn pack_id(&self) -> &'static str {
        "en-guard"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<radix_lexer::TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "assert" => Some("adfirma"),
            "require" => Some("requirit"),
            "reject" => Some("reice"),
            "throw" => Some("iace"),
            "panic" => Some("mori"),
            "else" => Some("secus"),
            "true" => Some("verum"),
            "for" => Some("itera"),
            "from" => Some("ex"),
            "range" => Some("ab"),
            "at" => Some("apud"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "fn" => Some("functio"),
            "size" => Some("magnitudo"),
            "coalesce" => Some("vel"),
            "void" => Some("vacuum"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnGuardPack))
}

fn parse_ok_en(source: &str) -> ParseResult {
    let result = parse_en(source);
    assert!(
        result.success(),
        "expected en-surface parse success, got errors: {:?}",
        result.errors
    );
    result
}

fn assert_en_parse_error_issue(source: &str, issue: &str) {
    let result = parse_en(source);
    let errors = result.errors.iter().collect::<Vec<_>>();
    assert!(
        result.errors.iter().any(|err| has_arg(err, "issue", issue)),
        "expected en-surface parse error issue {issue:?}, got {errors:?}"
    );
}

/// Minimal en surface for mutability- and declaration-expectation args
/// (`const`/`var`, `fn`, the praepara hook family, and the scribe/incipit
/// statement family).
struct SurfaceArgsPack;

impl KeywordPack for SurfaceArgsPack {
    fn pack_id(&self) -> &'static str {
        "en-surface-args"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "mut" => Some("varia"),
            "fn" => Some("functio"),
            "probandum" => Some("probandum"),
            "setup" => Some("praepara"),
            "async_setup" => Some("praeparabit"),
            "teardown" => Some("postpara"),
            "async_teardown" => Some("postparabit"),
            "write" => Some("scribe"),
            "debug" => Some("vide"),
            "warn" => Some("mone"),
            "print" => Some("nota"),
            "main" => Some("incipit"),
            "async_main" => Some("incipiet"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        match latin_key {
            "fixum" => Some("const"),
            "varia" => Some("var"),
            "functio" => Some("fn"),
            "probandum" => Some("probandum"),
            "praepara" => Some("setup"),
            "praeparabit" => Some("async_setup"),
            "postpara" => Some("teardown"),
            "postparabit" => Some("async_teardown"),
            "scribe" => Some("write"),
            "vide" => Some("debug"),
            "mone" => Some("warn"),
            "nota" => Some("print"),
            "incipit" => Some("main"),
            "incipiet" => Some("async_main"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_surface_args(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &SurfaceArgsPack))
}

/// Value of the named structured diagnostic arg, if present.
fn issue_arg<'a>(err: &'a ParseError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

/// Parser over `source` under [`SurfaceArgsPack`], keeping the reader maps the
/// lexer recorded (the same wiring `parse_with_options` installs).
fn surface_args_parser(source: &str) -> crate::Parser {
    let lexed = lex_with_locale_pack(source, &SurfaceArgsPack);
    assert!(
        lexed.errors.is_empty(),
        "surface-args source must lex cleanly: {:?}",
        lexed.errors
    );
    crate::Parser::new_with_reader_identities(
        lexed.tokens,
        lexed.interner,
        crate::ParseOptions::default(),
        lexed.reader_keyword_latin_keys,
        lexed.reader_keyword_surfaces,
        lexed.reader_type_latin_keys,
        lexed.reader_type_surfaces,
        lexed.keyword_surface,
    )
}

/// Parser over `source` on the plain Latin surface (no reader pack).
fn plain_parser(source: &str) -> crate::Parser {
    let lexed = radix_lexer::lex(source);
    crate::Parser::new(lexed.tokens, lexed.interner, crate::ParseOptions::default())
}

#[test]
fn parser_mutability_expectation_issue_carries_surface_args() {
    // Control: the pack surface installs the reader maps the expectation path
    // reads, and a valid `const` declaration parses through them.
    let ok = parse_surface_args("const _ x = 1");
    assert!(
        ok.success(),
        "const declaration must parse on the en surface: {:?}",
        ok.errors
    );

    // `parse_stmt` routes to `parse_var_decl` only when the current word
    // already claims `fixum`/`varia`, so the defensive miss branch is reached
    // by calling the declaration parser at a non-keyword cursor.
    let mut parser = surface_args_parser("numerus x");
    let Err(error) = parser.parse_var_decl() else {
        panic!("non-keyword cursor must not parse as a var declaration");
    };
    assert_eq!(
        issue_arg(&error, "issue"),
        Some("expected_mutability_keyword")
    );
    assert_eq!(issue_arg(&error, "fixum"), Some("const"));
    assert_eq!(issue_arg(&error, "varia"), Some("var"));

    let mut plain = plain_parser("numerus x");
    let Err(plain_error) = plain.parse_var_decl() else {
        panic!("non-keyword cursor must not parse as a var declaration");
    };
    assert_eq!(
        issue_arg(&plain_error, "issue"),
        Some("expected_mutability_keyword")
    );
    assert_eq!(issue_arg(&plain_error, "fixum"), Some("fixum"));
    assert_eq!(issue_arg(&plain_error, "varia"), Some("varia"));
}

#[test]
fn parser_praepara_hook_expectation_issue_carries_surface_args() {
    // Two declaration expectations guard code no source reaches, so this
    // pins each site's exact keyword list through the helper the sites call:
    //
    // - `parse_class_member`'s `expected_method_declaration` else-arm only
    //   runs when `parse_func_decl` returns an `Ok` value that is not
    //   `StmtKind::Func`; that parser has a single successful exit and always
    //   builds `StmtKind::Func`, so the arm is unreachable.
    // - `parse_probandum_body`'s `expected_praepara_hook` arm only runs after
    //   `check_keyword_ident` matched one of the four hook identities and all
    //   four `eat_keyword_ident` calls then missed; check and eat resolve the
    //   same token through the same claim path, so that arm is unreachable.
    //
    // The args themselves stay load-bearing: the en pack rows render
    // `{functio}`/`{praepara}`/... and must not hard-code Latin keywords into
    // a non-Latin reader surface.
    //
    // Control: the pack claims the praepara family, and a `setup` hook parses
    // through the same check/eat path the expectation guards.
    let ok = parse_surface_args("probandum \"pack\" { setup { } }");
    assert!(
        ok.success(),
        "praepara hook must parse on the en surface: {:?}",
        ok.errors
    );

    let parser = surface_args_parser("setup");
    let span = parser.current_span();

    let method_error =
        parser.expected_issue_with_keywords("expected_method_declaration", span, &["functio"]);
    assert_eq!(
        issue_arg(&method_error, "issue"),
        Some("expected_method_declaration")
    );
    assert_eq!(issue_arg(&method_error, "functio"), Some("fn"));

    let hook_error = parser.expected_issue_with_keywords(
        "expected_praepara_hook",
        span,
        &["praepara", "praeparabit", "postpara", "postparabit"],
    );
    assert_eq!(
        issue_arg(&hook_error, "issue"),
        Some("expected_praepara_hook")
    );
    assert_eq!(issue_arg(&hook_error, "praepara"), Some("setup"));
    assert_eq!(issue_arg(&hook_error, "praeparabit"), Some("async_setup"));
    assert_eq!(issue_arg(&hook_error, "postpara"), Some("teardown"));
    assert_eq!(
        issue_arg(&hook_error, "postparabit"),
        Some("async_teardown")
    );
}

#[test]
fn parser_scribe_incipit_expectation_issues_carry_surface_args() {
    // `parse_stmt` routes to `parse_scribe_stmt`/`parse_incipit_stmt` only
    // after `current_keyword_ident` already matched one of the family's
    // keywords, and each parser consumes that same identity through the same
    // claim path, so the miss branches are defensive (unreachable from
    // source). This pins each site's exact keyword list and the pack surface
    // the helper records: the en pack rows render
    // `{scribe}`/`{vide}`/`{incipit}`/... and must not hard-code Latin
    // keywords into a non-Latin reader surface.
    //
    // Control: the pack claims the statement families, and valid statements
    // parse through the same check/eat path the expectations guard.
    let scribe_ok = parse_surface_args("write \"pack\"");
    assert!(
        scribe_ok.success(),
        "scribe statement must parse on the en surface: {:?}",
        scribe_ok.errors
    );
    let incipit_ok = parse_surface_args("main { }");
    assert!(
        incipit_ok.success(),
        "incipit statement must parse on the en surface: {:?}",
        incipit_ok.errors
    );

    let mut scribe_parser = surface_args_parser("numerus x");
    let Err(scribe_error) = scribe_parser.parse_scribe_stmt() else {
        panic!("non-keyword cursor must not parse as a scribe statement");
    };
    assert_eq!(
        issue_arg(&scribe_error, "issue"),
        Some("expected_scribe_keyword")
    );
    assert_eq!(issue_arg(&scribe_error, "scribe"), Some("write"));
    assert_eq!(issue_arg(&scribe_error, "vide"), Some("debug"));
    assert_eq!(issue_arg(&scribe_error, "mone"), Some("warn"));
    assert_eq!(issue_arg(&scribe_error, "nota"), Some("print"));

    let mut incipit_parser = surface_args_parser("numerus x");
    let Err(incipit_error) = incipit_parser.parse_incipit_stmt() else {
        panic!("non-keyword cursor must not parse as an incipit statement");
    };
    assert_eq!(
        issue_arg(&incipit_error, "issue"),
        Some("expected_incipit_keyword")
    );
    assert_eq!(issue_arg(&incipit_error, "incipit"), Some("main"));
    assert_eq!(issue_arg(&incipit_error, "incipiet"), Some("async_main"));
}

#[test]
fn parses_itera_apud_clause_on_en_at_surface() {
    let result = parse_ok_en("for from m at [i, j] let cell { }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    let coordinates = iter_stmt
        .apud
        .iter()
        .map(|ident| symbol_name(&result, ident.name))
        .collect::<Vec<_>>();
    assert_eq!(coordinates, ["i", "j"]);
}

#[test]
fn parses_en_for_range_series_at_surface() {
    // The documented English spelling of the NR-1 `ab` range series parses
    // through the same source/binder series shape (`en` pack: range = ab).
    let result = parse_ok_en("for range 0‥m, 0‥n, 0‥k let i, j, p { }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    assert_eq!(
        std::mem::discriminant(&iter_stmt.mode),
        std::mem::discriminant(&IteraMode::Ab)
    );
    assert_eq!(iter_stmt.sources.len(), 3);
    assert_eq!(iter_stmt.bindings.len(), 3);
}

#[test]
fn parses_adfirma_assert() {
    let result = parse_ok("adfirma verum mori \"msg\"");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Adfirma(_)));

    let en = parse_ok_en("assert true panic \"msg\"");
    let en_program = en
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(en_program.statements.len(), 1);
    assert!(matches!(
        en_program.statements[0].kind,
        StmtKind::Adfirma(_)
    ));
}

#[test]
fn rejects_adfirma_comma_message() {
    assert_parse_error_matches(
        "adfirma verum, \"msg\"",
        ParseExpectation::Args(&[("issue", "expected_expression")]),
    );
}

#[test]
fn parses_adfirma_without_message() {
    let result = parse_ok("adfirma verum");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Adfirma(_)));
}

#[test]
fn parses_requirit_statement() {
    let result = parse_ok("requirit verum iace \"bad\"");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    let StmtKind::Require(require_stmt) = &program.statements[0].kind else {
        panic!("expected requirit statement");
    };
    assert!(
        matches!(
            require_stmt.cond.kind,
            ExprKind::KeywordLiteral(radix_syntax::KeywordLiteralExpr {
                literal: Literal::Bool(true),
                ..
            })
        ),
        "cond should be the boolean literal verum"
    );
    assert!(matches!(require_stmt.error.kind, ExprKind::Literal(_)));

    let en = parse_ok_en("require true throw \"bad\"");
    let en_program = en.program.as_ref().expect("program");
    assert_eq!(en_program.statements.len(), 1);
    assert!(matches!(
        en_program.statements[0].kind,
        StmtKind::Require(_)
    ));
}

#[test]
fn value_literal_keywords_keep_their_spelling() {
    // The resolver needs the written spelling to let a same-named binding win.
    for (source, keyword) in [
        ("reice verum iace \"bad\"", "verum"),
        ("reice falsum iace \"bad\"", "falsum"),
        ("reice nulla iace \"bad\"", "nulla"),
        ("reice nonnumerus iace \"bad\"", "nonnumerus"),
    ] {
        let result = parse_ok(source);
        let program = result.program.as_ref().expect("program");
        let StmtKind::Reject(reject_stmt) = &program.statements[0].kind else {
            panic!("expected reice statement");
        };
        let ExprKind::KeywordLiteral(literal) = &reject_stmt.cond.kind else {
            panic!(
                "{keyword} should parse to a keyword literal: {:?}",
                reject_stmt.cond.kind
            );
        };
        assert_eq!(result.interner.resolve(literal.ident.name), keyword);
    }
}

#[test]
fn parses_reice_statement() {
    let result = parse_ok("reice verum iace \"bad\"");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    let StmtKind::Reject(reject_stmt) = &program.statements[0].kind else {
        panic!("expected reice statement");
    };
    assert!(
        matches!(
            reject_stmt.cond.kind,
            ExprKind::KeywordLiteral(radix_syntax::KeywordLiteralExpr {
                literal: Literal::Bool(true),
                ..
            })
        ),
        "cond should be the boolean literal verum"
    );
    assert!(matches!(reject_stmt.error.kind, ExprKind::Literal(_)));

    let en = parse_ok_en("reject true throw \"bad\"");
    let en_program = en.program.as_ref().expect("program");
    assert_eq!(en_program.statements.len(), 1);
    assert!(matches!(en_program.statements[0].kind, StmtKind::Reject(_)));
}

#[test]
fn rejects_reice_without_secus_payload() {
    assert_parse_error_matches(
        "reice verum",
        ParseExpectation::Args(&[("issue", "reice_iace")]),
    );
}

#[test]
fn rejects_reice_else() {
    assert_parse_error_matches(
        "reice verum secus \"bad\"",
        ParseExpectation::Args(&[("issue", "reice_else_rejected")]),
    );
    assert_en_parse_error_issue("reject true else \"bad\"", "reice_else_rejected");
}

#[test]
fn parses_requirit_error_expression_forms() {
    let result =
        parse_ok(r#"requirit conditio.finita iace finge SchedulaInvalida { causa = "bad" }"#);
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Require(_)));
}

#[test]
fn rejects_requirit_without_secus_payload() {
    assert_parse_error_matches(
        "requirit verum",
        ParseExpectation::Args(&[("issue", "requirit_iace")]),
    );
}

#[test]
fn rejects_requirit_else() {
    assert_parse_error_matches(
        "requirit verum secus \"bad\"",
        ParseExpectation::Args(&[("issue", "requirit_else_rejected")]),
    );
    assert_en_parse_error_issue("require true else \"bad\"", "requirit_else_rejected");
}

#[test]
fn rejects_adfirma_else() {
    assert_parse_error_matches(
        "adfirma verum secus \"msg\"",
        ParseExpectation::Args(&[("issue", "adfirma_else_rejected")]),
    );
    assert_en_parse_error_issue("assert true else \"msg\"", "adfirma_else_rejected");
}

#[test]
fn rejects_adfirma_message_without_mori() {
    assert_parse_error_matches(
        "adfirma 1 > 2 \"msg\"",
        ParseExpectation::Args(&[("issue", "adfirma_message_particle")]),
    );
    assert_en_parse_error_issue("assert 1 > 2 \"boom\"", "adfirma_message_particle");
}

#[test]
fn adfirma_string_on_next_line_stays_a_separate_statement() {
    let result = parse_ok("adfirma verum\n\"msg\"");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);
}

#[test]
fn parses_nota_diagnostic() {
    let result = parse_ok("nota \"a\", \"b\"");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0].kind {
        StmtKind::Scribe(stmt) => assert!(matches!(stmt.kind, ScribeKind::Nota)),
        _ => panic!("expected nota statement"),
    }
}

#[test]
fn parses_vide_diagnostic() {
    let result = parse_ok("vide \"c\"");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0].kind {
        StmtKind::Scribe(stmt) => assert!(matches!(stmt.kind, ScribeKind::Vide)),
        _ => panic!("expected vide statement"),
    }
}

#[test]
fn parses_mone_diagnostic() {
    let result = parse_ok("mone \"d\"");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0].kind {
        StmtKind::Scribe(stmt) => assert!(matches!(stmt.kind, ScribeKind::Mone)),
        _ => panic!("expected mone statement"),
    }
}

#[test]
fn parses_scribe_diagnostic() {
    let result = parse_ok("scribe \"e\"");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0].kind {
        StmtKind::Scribe(stmt) => assert!(matches!(stmt.kind, ScribeKind::Scribe)),
        _ => panic!("expected scribe compatibility statement"),
    }
}

/// KW-U1: bare `nota 5` stays the output statement (next token is not `(`).
#[test]
fn statement_initial_bare_nota_stays_scribe() {
    let result = parse_ok("nota 5");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0].kind {
        StmtKind::Scribe(stmt) => assert!(matches!(stmt.kind, ScribeKind::Nota)),
        other => panic!("expected scribe statement, got {other:?}"),
    }
}

/// KW-U1: `nota (5)` at statement start is a user call, not the output statement.
#[test]
fn statement_initial_nota_paren_form_is_a_call() {
    let result = parse_ok("nota (5)");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    let StmtKind::Expr(expr_stmt) = &program.statements[0].kind else {
        panic!(
            "expected expression statement, got {:?}",
            program.statements[0].kind
        );
    };
    let ExprKind::Call(call) = &expr_stmt.expr.kind else {
        panic!("expected call, got {:?}", expr_stmt.expr.kind);
    };
    let ExprKind::Ident(ident) = &call.callee.kind else {
        panic!("expected ident callee, got {:?}", call.callee.kind);
    };
    assert_eq!(symbol_name(&result, ident.name), "nota");
}

/// KW-U1: the paren lookahead applies only to the scribe family.
#[test]
fn statement_initial_si_with_paren_condition_stays_if() {
    let result = parse_ok("si (x) { }");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    assert!(
        matches!(program.statements[0].kind, StmtKind::Si(_)),
        "structural `si (x) {{ }}` must stay an if-statement, got {:?}",
        program.statements[0].kind
    );
}

#[test]
fn parses_redde_return() {
    let result = parse_ok("redde 1");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Redde(_)));
}

#[test]
fn parses_bare_redde_return() {
    let result = parse_ok("redde");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Redde(_)));
}

#[test]
fn bare_redde_ends_at_newline() {
    let result = parse_ok("redde\nf(1)");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);
    let StmtKind::Redde(redde) = &program.statements[0].kind else {
        panic!("expected redde, got {:?}", program.statements[0].kind);
    };
    assert!(
        redde.value.is_none(),
        "a newline must end a bare redde, not start its value"
    );
}

#[test]
fn parses_rumpe_break() {
    let result = parse_ok("rumpe");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Rumpe(_)));
}

#[test]
fn parses_perge_continue() {
    let result = parse_ok("perge");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Perge(_)));
}

#[test]
fn parses_iace_throw() {
    let result = parse_ok("iace \"err\"");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Iace(_)));
}

#[test]
fn parses_mori_panic() {
    let result = parse_ok("mori \"panic\"");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Mori(_)));
}

#[test]
fn parses_tacet_noop() {
    let result = parse_ok("tacet");
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Tacet(_)));
}

#[test]
fn invalid_discerne_pattern_emits_structured_parse_error() {
    let result = parse_program("discerne value { casu + ergo tacet }");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::PatternInvalid)
        .expect("invalid pattern diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 2);
    assert_eq!(err.args[0].name, "wildcard");
    assert_eq!(err.args[0].value, "_");
    assert_eq!(err.args[1].name, "identifier");
    assert_eq!(err.args[1].value, "identifier");
}

#[test]
fn elige_missing_casu_rejected() {
    let result = parse_program("elige value { tacet }");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::CasuArmInvalid)
        .expect("invalid casu arm diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 2);
    assert_eq!(err.args[0].name, "casu");
    assert_eq!(err.args[0].value, "casu");
    assert_eq!(err.args[1].name, "ceterum");
    assert_eq!(err.args[1].value, "ceterum");
}

#[test]
fn discerne_missing_casu_rejected() {
    let result = parse_program("discerne value { tacet }");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::CasuArmInvalid)
        .expect("invalid casu arm diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 2);
    assert_eq!(err.args[0].name, "casu");
    assert_eq!(err.args[0].value, "casu");
    assert_eq!(err.args[1].name, "ceterum");
    assert_eq!(err.args[1].value, "ceterum");
}

#[test]
fn parses_ergo_keyword_as_statement_tail() {
    let result = parse_ok(
        r#"
si verum ergo nota "yes"
dum falsum ergo tacet
itera ab items sit item ergo redde item
"#,
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 3);
    let StmtKind::Si(stmt) = &program.statements[0].kind else {
        panic!("expected si statement");
    };
    assert!(matches!(stmt.then, IfBody::Ergo(_)));
    let StmtKind::Dum(stmt) = &program.statements[1].kind else {
        panic!("expected dum statement");
    };
    assert!(matches!(stmt.body, IfBody::Ergo(_)));
}

#[test]
fn rejects_therefore_glyph_as_statement_body_joint() {
    let result = parse_program(
        r#"
si verum ∴ nota "yes"
"#,
    );
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == ParseErrorKind::ThereforeWrongContext),
        "expected ThereforeWrongContext, got {:?}",
        result.errors
    );
}

#[test]
fn rejects_ergo_keyword_as_clausura_joint() {
    let result = parse_program(
        r"
incipit {
    sit _ dupla ← numerus x ergo x * 2
}
",
    );
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == ParseErrorKind::ErgoWrongContext),
        "expected ErgoWrongContext, got {:?}",
        result.errors
    );
}

#[test]
fn parses_block_si_when_condition_ends_with_star_textus_literal() {
    let result = parse_ok(
        r#"
functio f(textus segmentum) → vacuum {
    si segmentum ≡ "*" {
        nota "star"
    } secus {
        nota "literal"
    }
}
"#,
    );

    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("function body");
    assert!(matches!(body.statements[0].kind, StmtKind::Si(_)));
}

#[test]
fn rejects_itera_pro_mode() {
    assert_parse_error_matches(
        r"itera pro 0‥3 fixum i { nota i }",
        ParseExpectation::Arg {
            name: "expected",
            value: "'ex', 'de', or 'ab'",
        },
    );
}

#[test]
fn parses_single_param_implicit_type_closure() {
    let result = parse_ok("sit _ a ← users.filtrata(_ user ∴ non user.activus)");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable");
    };
    let ExprKind::Call(call) = &var.init.as_ref().expect("init").kind else {
        panic!("expected call");
    };
    let ExprKind::Clausura(closure) = &call.args[0].value.kind else {
        panic!("expected compact closure");
    };
    assert!(matches!(closure.body, ClausuraBody::Expr(_)));
}

#[test]
fn parses_single_param_explicit_type_closure() {
    let result = parse_ok("sit _ b ← users.filtrata(User user ∴ non user.activus)");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable");
    };
    let ExprKind::Call(call) = &var.init.as_ref().expect("init").kind else {
        panic!("expected call");
    };
    let ExprKind::Clausura(closure) = &call.args[0].value.kind else {
        panic!("expected compact closure");
    };
    assert!(matches!(closure.body, ClausuraBody::Expr(_)));
}

#[test]
fn parses_multi_param_implicit_type_closure() {
    let result = parse_ok("sit _ c ← numeri.compone((_ a, _ b) ∴ a + b)");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable");
    };
    let ExprKind::Call(call) = &var.init.as_ref().expect("init").kind else {
        panic!("expected call");
    };
    let ExprKind::Clausura(closure) = &call.args[0].value.kind else {
        panic!("expected compact closure");
    };
    assert!(matches!(closure.body, ClausuraBody::Expr(_)));
}

#[test]
fn parses_multi_param_explicit_type_fac_closure() {
    let result = parse_ok("sit _ d ← numeri.compone((numerus a, numerus b) ∴ fac { redde a + b })");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(block_var) = &program.statements[0].kind else {
        panic!("expected variable");
    };
    let ExprKind::Call(call) = &block_var.init.as_ref().expect("init").kind else {
        panic!("expected call");
    };
    let ExprKind::Clausura(closure) = &call.args[0].value.kind else {
        panic!("expected compact closure");
    };
    assert_eq!(closure.params.len(), 2);
    assert!(matches!(closure.body, ClausuraBody::Fac(_)));
}

#[test]
fn parses_fallible_typed_closure() {
    let result = parse_ok(
        "sit _ e ← texts.mappata(textus s → numerus ⇥ ParseError ∴ fac { redde parse(s) } cape err { redde 0 })",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(fallible_var) = &program.statements[0].kind else {
        panic!("expected variable");
    };
    let ExprKind::Call(call) = &fallible_var.init.as_ref().expect("init").kind else {
        panic!("expected call");
    };
    let ExprKind::Clausura(closure) = &call.args[0].value.kind else {
        panic!("expected compact closure");
    };
    assert!(closure.ret.is_some());
    assert!(closure.err.is_some());
    assert!(matches!(closure.body, ClausuraBody::Fac(_)));
}

#[test]
fn closure_block_body_rejected_with_structured_error() {
    let result =
        parse_program("sit _ inactive ← users.filtrata(_ user ∴ { redde non user.activus })");
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::ExpressionInvalid
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "closure_block_body")
        })
        .expect("invalid expression diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "fac_body" && arg.value == "fac { ... }"),
        "closure block diagnostic should expose pack-safe repair text, got {:?}",
        err.args
    );
}

#[test]
fn closure_fac_body_dum_rejected() {
    assert_parse_error_matches(
        "sit _ inactive ← users.filtrata(_ user ∴ fac { redde non user.activus } dum user.activus)",
        ParseExpectation::Issue("closure_fac_body_dum"),
    );
}

#[test]
fn fac_cape_after_dum_rejected_at_cape() {
    // Grammar order is `fac block cape_clause? (dum expression)?`; a handler
    // written after the loop condition is a parse error at the `cape` token,
    // never a stray `cape err { … }` statement of unknown identifiers.
    let source = "fac { tacet } dum verum cape err { tacet }";
    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::StatementInvalid
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "fac_cape_after_dum")
        })
        .unwrap_or_else(|| panic!("expected fac_cape_after_dum, got {:?}", result.errors));
    assert_parse_error_has_no_legacy_message(err);
    let cape_at = u32::try_from(source.find("cape").expect("cape token")).expect("offset fits");
    assert_eq!(
        err.span.start, cape_at,
        "diagnostic must point at the cape token"
    );
}

#[test]
fn parses_structured_cape_attachment_targets() {
    let result = parse_ok(
        r"
si verum { tacet } cape err {} sin falsum { tacet } cape err {} secus { tacet } cape err {}
dum verum { tacet } cape err {}
fac { tacet } cape err {}
",
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 3);

    let StmtKind::Si(si_stmt) = &program.statements[0].kind else {
        panic!("expected si");
    };
    assert!(si_stmt.catch.is_some());
    let SecusClause::Sin {
        stmt: else_if_stmt, ..
    } = si_stmt.else_.as_ref().expect("sin")
    else {
        panic!("expected sin");
    };
    assert!(else_if_stmt.catch.is_some());
    let SecusClause::Block { catch, .. } = else_if_stmt.else_.as_ref().expect("secus") else {
        panic!("expected secus block");
    };
    assert!(catch.is_some());

    let StmtKind::Dum(dum_stmt) = &program.statements[1].kind else {
        panic!("expected dum");
    };
    assert!(dum_stmt.catch.is_some());

    let StmtKind::Fac(fac_stmt) = &program.statements[2].kind else {
        panic!("expected fac");
    };
    assert!(fac_stmt.catch.is_some());
}

#[test]
fn cape_keyword_usable_as_identifier_after_block() {
    // No reserved words: `cape` (catch) is an ordinary identifier outside its
    // recovery-clause slot, so it is a valid expression statement.
    let result = parse_ok(r"functio f() → vacuum { { tacet } cape }");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
}

#[test]
fn removed_spellings_are_usable_as_identifiers() {
    // The retired spellings become ordinary identifiers where the grammar
    // permits identifiers: function names, binding names, and members.
    let result = parse_ok(
        r"
functio tempta() → vacuum { tacet }
functio demum() → vacuum { tacet }
functio emitte() → vacuum { tacet }
functio pro() → vacuum { tacet }
incipit {
  sit _ pro ← 1
  sit _ tempta ← 2
  nota tempta
}
",
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 5);
}

#[test]
fn modulus_type_and_itera_ab_range_iteration_stay_live() {
    // Positive regression proof: `modulus<W>` types and `itera ab <range>`
    // range iteration are canonical and must keep parsing after the removals.
    let result = parse_ok(
        r"
functio checksum() → numerus {
  sit modulus<u32> m ← 1779033703
  itera ab 0‥10 per 2 sit i {
    nota i
  }
  redde 0
}
",
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
}

#[test]
fn migrated_keywords_usable_as_contextual_identifiers() {
    // "Contextual" means a word is only special inside its grammar slot —
    // everywhere else it is an ordinary identifier. Each migrated keyword
    // (Batches A–B) is proven usable in name positions (variable, function,
    // parameter) and as a member name. Statement/type-position words are also
    // free in value position; expression-primary words (`iuncta`, `lege`,
    // `lineam`, `scriptum`) are intentionally claimed at expression start, so
    // a bare value reference is not asserted for them.
    let words = [
        "modulus",
        "iuncta",
        "nota",
        "mone",
        "vide",
        "lege",
        "lineam",
        "scriptum",
        "verum",
        "falsum",
        "nihil",
        "aut",
        "et",
        "non",
        "est",
        "vel",
        "functio",
        "genus",
        "ordo",
        "implendum",
        "typus",
        "discretio",
        "fixum",
        "varia",
        "sit",
        "si",
        "secus",
        "sin",
        "ergo",
        "discerne",
        "elige",
        "dum",
        "fac",
        "casu",
        "ceterum",
        "rumpe",
        "perge",
        "redde",
        "ego",
        "finge",
        "implet",
        "ad",
        "omnia",
        "inter",
        "intra",
        "per",
        "usque",
        "ante",
        "praefixum",
        "clausura",
        "sponte",
        "tacet",
    ];
    let name_position_templates = [
        "sit _ {word} ← 5\n",
        "functio {word}() → numerus {{ redde 0 }}\n",
        "functio f(numerus {word}) → numerus {{ redde 0 }}\n",
        "incipit {{ sit _ x ← 5\nnota x.{word}\n}}\n",
    ];
    for word in words {
        for template in name_position_templates {
            let source = template.replace("{word}", word);
            let result = parse_program(&source);
            assert!(
                result.success(),
                "keyword '{word}' must be usable as an identifier:\n{source}\nerrors: {:?}",
                result.errors
            );
        }
    }
    // Value position: words not claimed at expression-primary. `non` is
    // excluded — unary `non` claims it at expression start, so a bare value
    // reference is ambiguous (same tradeoff as expression-primary words).
    for word in [
        "modulus",
        "nota",
        "mone",
        "vide",
        "aut",
        "et",
        "est",
        "vel",
        "functio",
        "genus",
        "ordo",
        "implendum",
        "typus",
        "discretio",
        "fixum",
        "varia",
        "sit",
        "si",
        "secus",
        "sin",
        "ergo",
        "discerne",
        "elige",
        "dum",
        "fac",
        "casu",
        "ceterum",
        "rumpe",
        "perge",
        "redde",
        "implet",
        "omnia",
        "inter",
        "intra",
        "per",
        "usque",
        "ante",
        "sponte",
        "tacet",
    ] {
        let source = format!("functio f() → numerus {{ sit _ {word} ← 5\nredde {word}\n}}\n");
        let result = parse_program(&source);
        assert!(
            result.success(),
            "keyword '{word}' must be usable in value position:\n{source}\nerrors: {:?}",
            result.errors
        );
    }
}

#[test]
fn statement_level_ad_rejected_with_structured_error() {
    let result = parse_program("incipit { ad 'noop:ping' { meus { } } }");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::StatementInvalid)
        .expect("invalid statement diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 3);
    assert_eq!(err.args[0].name, "issue");
    assert_eq!(err.args[0].value, "ad_expression_only");
    assert_eq!(err.args[1].name, "statement");
    assert_eq!(err.args[1].value, "ad");
    assert_eq!(err.args[2].name, "replacement");
    assert_eq!(err.args[2].value, "ad 'route' (opener) ↦ T expression form");
    assert_eq!(
        err.span,
        span_for("incipit { ad 'noop:ping' { meus { } } }", "ad")
    );
}

#[test]
fn legacy_ad_route_expression_rejected_with_structured_error() {
    let source = r#"incipit { ad "legacy:route" () → textus x ⇥ textus { } }"#;
    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::StatementInvalid)
        .expect("invalid statement diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 3);
    assert_eq!(err.args[0].name, "issue");
    assert_eq!(err.args[0].value, "ad_expression_only");
    assert_eq!(err.args[1].name, "statement");
    assert_eq!(err.args[1].value, "ad");
    assert_eq!(err.args[2].name, "replacement");
    assert_eq!(err.args[2].value, "ad 'route' (opener) ↦ T expression form");
    assert_eq!(err.span, span_for(source, "ad"));
}

#[test]
fn si_without_block_or_ergo_rejected() {
    let result = parse_program("si verum tacet");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::BlockAbsent)
        .expect("missing block diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 2);
    assert_eq!(err.args[0].name, "block");
    assert_eq!(err.args[0].value, "{ ... }");
    assert_eq!(err.args[1].name, "ergo");
    assert_eq!(err.args[1].value, "ergo");
}

#[test]
fn si_secus_without_block_or_ergo_rejected() {
    let result = parse_program("si verum ergo tacet secus tacet");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::BlockAbsent)
        .expect("missing block diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 2);
    assert_eq!(err.args[0].name, "block");
    assert_eq!(err.args[0].value, "{ ... }");
    assert_eq!(err.args[1].name, "ergo");
    assert_eq!(err.args[1].value, "ergo");
}

#[test]
fn elige_without_block_rejected() {
    let result = parse_program("elige value { casu 1 tacet }");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::BlockAbsent)
        .expect("missing block diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 2);
    assert_eq!(err.args[0].name, "block");
    assert_eq!(err.args[0].value, "{ ... }");
    assert_eq!(err.args[1].name, "ergo");
    assert_eq!(err.args[1].value, "ergo");
}

#[test]
fn dum_without_block_rejected() {
    let result = parse_program("dum verum tacet");
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::BlockAbsent)
        .expect("missing block diagnostic");

    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args.len(), 2);
    assert_eq!(err.args[0].name, "block");
    assert_eq!(err.args[0].value, "{ ... }");
    assert_eq!(err.args[1].name, "ergo");
    assert_eq!(err.args[1].value, "ergo");
}

#[test]
fn incipit_tacet_expected_block() {
    assert_parse_error_matches(
        "incipit tacet",
        ParseExpectation::Arg {
            name: "issue",
            value: "block_start",
        },
    );
}

#[test]
fn parses_entry_endpoint_and_extract_keywords() {
    let result = parse_ok(
        r"
incipit argumenta args exitus 1 {}
incipiet argumenta argv {}
ex source varia nomen ut name, ceteri reliqua
",
    );

    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    assert_eq!(program.statements.len(), 3);

    let StmtKind::Incipit(main_stmt) = &program.statements[0].kind else {
        panic!("expected incipit statement");
    };
    assert!(!main_stmt.is_async);
    assert!(main_stmt.args.is_some());
    assert!(main_stmt.exitus.is_some());
    assert!(matches!(main_stmt.body, IfBody::Block(_)));

    let StmtKind::Incipit(async_main) = &program.statements[1].kind else {
        panic!("expected incipiet statement");
    };
    assert!(async_main.is_async);
    assert!(matches!(async_main.body, IfBody::Block(_)));

    let StmtKind::Ex(extract) = &program.statements[2].kind else {
        panic!("expected ex destructuring statement");
    };
    assert!(matches!(extract.mutability, AstMutability::Mutable));
    assert_eq!(extract.fields.len(), 1);
    assert_eq!(
        symbol_name(
            &result,
            extract.fields[0].alias.as_ref().expect("field alias").name
        ),
        "name"
    );
    assert_eq!(
        symbol_name(
            &result,
            extract.is_rest.as_ref().expect("rest binding").name
        ),
        "reliqua"
    );
}

#[test]
fn parses_function_alternate_exit_type() {
    let result = parse_ok(r"functio divide(numerus a, numerus b) → numerus ⇥ textus { redde a }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    assert!(matches!(
        func.ret.as_ref().map(|ty| &ty.kind),
        Some(TypeExprKind::Named(_, _))
    ));
    assert!(matches!(
        func.err.as_ref().map(|ty| &ty.kind),
        Some(TypeExprKind::Named(_, _))
    ));
    let Some(err) = &func.err else {
        panic!("expected alternate-exit type");
    };
    let TypeExprKind::Named(name, params) = &err.kind else {
        panic!("expected named alternate-exit type");
    };
    assert!(params.is_empty());
    assert_eq!(symbol_name(&result, name.name), "textus");
}

#[test]
fn parses_implendum_method_alternate_exit_type() {
    let result = parse_ok(
        r"
implendum Divisor {
  functio divide(numerus a, numerus b) → numerus ⇥ textus
}
",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Interface(interface) = &program.statements[0].kind else {
        panic!("expected implendum declaration");
    };
    assert_eq!(interface.methods.len(), 1);
    assert!(interface.methods[0].ret.is_some());
    assert!(interface.methods[0].err.is_some());
}

#[test]
fn preserves_implendum_method_annotations() {
    let result = parse_ok(
        r#"
implendum Horologium {
  @ nondum rs "timer handles are not implemented"
  functio repete(numerus ms) → vacuum
}
"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Interface(interface) = &program.statements[0].kind else {
        panic!("expected implendum declaration");
    };
    assert_eq!(interface.methods.len(), 1);
    assert_eq!(interface.methods[0].annotations.len(), 1);
    let AnnotationKind::Statement(annotation) = &interface.methods[0].annotations[0].kind else {
        panic!("expected statement annotation");
    };
    assert_eq!(symbol_name(&result, annotation.name.name), "nondum");
    assert_eq!(annotation.args.len(), 2);
}

#[test]
fn parses_failable_function_type() {
    let result = parse_ok(r"typus Op = (numerus) → numerus ⇥ textus");
    let program = result.program.as_ref().expect("program");
    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias");
    };
    let TypeExprKind::Func(func) = &alias.ty.kind else {
        panic!("expected function type");
    };
    assert_eq!(func.params.len(), 1);
    assert!(func.err.is_some());
}

#[test]
fn param_binds_tokenless_keyword_as_identifier() {
    // `si` is tokenless since Batch F: `functio f(_ si)` names a parameter
    // `si`. Pre-migration the reserved Si token made any parameter named
    // `si` a parse error (`legacy_si_in_param_prefix_rejected`).
    let result = parse_ok("functio f(_ si) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    let Some(param) = func.params.first() else {
        panic!("expected a parameter");
    };
    assert_eq!(symbol_name(&result, param.name.name), "si");
}

#[test]
fn si_is_usable_as_a_type_name_after_tokenless_migration() {
    // `si` is tokenless since Batch F: it may name a type in any type
    // position. Pre-migration the reserved Si token made these parse errors
    // (`legacy_si_*_rejected` family).
    for source in [
        "sit si x ← nulla\n",
        "functio f(si handle) → vacuum {}\n",
        "functio find() → si { redde nulla }\n",
        "typus MaybeSi = si\n",
        "genus User { varia si email }\n",
    ] {
        let result = parse_program(source);
        assert!(
            result.success(),
            "'si' must be usable as a type name:\n{source}\nerrors: {:?}",
            result.errors
        );
    }
}

#[test]
fn varia_binds_tokenless_keyword_as_identifier() {
    // `si` is tokenless since Batch F: `varia _ si ← 5` binds a variable
    // named `si`. Pre-migration the reserved Si token made any binding named
    // `si` a parse error (`legacy_si_in_varia_declaration_rejected`).
    let result = parse_ok("varia _ si ← 5");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let BindingPattern::Ident(name) = &var.binding else {
        panic!("expected identifier binding");
    };
    assert_eq!(symbol_name(&result, name.name), "si");
}

#[test]
fn name_slots_accept_keyword_spellings_on_latin_surface() {
    // Formalized name-slot policy (KW-U3): declaration, parameter, and
    // binding-target slots accept every keyword spelling.
    for source in [
        "functio lege(textus lege) → vacuum {}\n",
        "functio nota(textus nota) → vacuum {}\n",
        "functio print(textus print) → vacuum {}\n",
        "sit textus lege ← \"data\"\n",
        "sit textus nota ← \"print\"\n",
        "sit textus print ← \"print\"\n",
    ] {
        let result = parse_program(source);
        assert!(
            result.success(),
            "name slot must accept keyword spelling:\n{source}\nerrors: {:?}",
            result.errors
        );
    }

    let func = parse_ok("functio lege(textus lege) → vacuum {}\n");
    let program = func.program.as_ref().expect("program");
    let StmtKind::Func(decl) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    assert_eq!(symbol_name(&func, decl.name.name), "lege");
    assert_eq!(symbol_name(&func, decl.params[0].name.name), "lege");

    let bind = parse_ok("sit textus lege ← \"data\"\n");
    let program = bind.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let BindingPattern::Ident(name) = &var.binding else {
        panic!("expected identifier binding");
    };
    assert_eq!(symbol_name(&bind, name.name), "lege");
}

#[test]
fn legacy_question_mark_in_fixum_rejected() {
    assert_parse_error_matches(
        r"sit textus? maybe ← nulla",
        ParseExpectation::Issue("expected_identifier"),
    );
}

#[test]
fn legacy_question_mark_in_function_return_rejected() {
    assert_parse_error_matches(
        r"functio find() → textus? { redde nulla }",
        ParseExpectation::Kind(ParseErrorKind::FunctionBodyAbsent),
    );
}

#[test]
fn legacy_question_mark_in_type_alias_rejected() {
    assert_parse_error_matches(
        r"typus MaybeText = textus?",
        ParseExpectation::Issue("expected_expression"),
    );
}

#[test]
fn fixus_in_param_rejected() {
    let result = parse_program(r"functio f(textus name fixus) → vacuum {}");
    assert!(
        !result.success(),
        "fixus parameter marker must be rejected, got {:?}",
        result.errors
    );
    assert_parse_error_has_no_legacy_message(result.errors.first().expect("parse diagnostic"));
}

#[test]
fn fixus_in_genus_field_rejected() {
    let result = parse_program(r"genus User { textus email fixus }");
    assert!(
        !result.success(),
        "fixus field marker must be rejected, got {:?}",
        result.errors
    );
    assert_parse_error_has_no_legacy_message(result.errors.first().expect("parse diagnostic"));
}

fn assert_parameter_access_modifier_pair_rejected(source: &str) {
    let result = parse_program(source);
    assert!(
        !result.success(),
        "parameter access-modifier pair must be rejected, got {:?}",
        result.errors
    );
    let err = result
        .errors
        .first()
        .expect("parameter access-modifier parse diagnostic");
    assert_parse_error_has_no_legacy_message(err);
}

#[test]
fn ref_mut_parameter_pair_is_parse_rejected() {
    assert_parameter_access_modifier_pair_rejected(
        "functio f(de in lista<numerus> values) → vacuum {}",
    );
}

#[test]
fn mut_ref_parameter_pair_is_parse_rejected() {
    assert_parameter_access_modifier_pair_rejected(
        "functio f(in de lista<numerus> values) → vacuum {}",
    );
}

#[test]
fn mut_mut_parameter_pair_is_parse_rejected() {
    assert_parameter_access_modifier_pair_rejected(
        "functio f(in in lista<numerus> values) → vacuum {}",
    );
}

fn assert_parameter_prefix_rejected(source: &str) {
    let result = parse_program(source);
    assert!(
        !result.success(),
        "parameter prefix must be a parse error, got {:?}",
        result.errors
    );
    assert_parse_error_has_no_legacy_message(
        result
            .errors
            .first()
            .expect("parameter prefix parse diagnostic"),
    );
}

#[test]
fn ex_parameter_prefix_is_invalid() {
    assert_parameter_prefix_rejected("functio consume(ex lista<numerus> values) → vacuum {}");
}

#[test]
fn from_parameter_prefix_is_invalid() {
    assert_parameter_prefix_rejected("functio consume(from string buffer) → string {}");
}

#[test]
fn parses_contextual_keywords_in_declaration_and_param_positions() {
    let result = parse_ok(
        r"
functio retorque(textus via) → textus { redde via }
functio genera(lista<textus> argumenta) → vacuum { tacet }
implendum Par {
    functio publica() → octeti
    functio privata() → octeti
    functio incipiet() → vacuum
}
",
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 3);
    let StmtKind::Func(retorque) = &program.statements[0].kind else {
        panic!("expected retorque function");
    };
    let StmtKind::Func(genera) = &program.statements[1].kind else {
        panic!("expected genera function");
    };
    assert_eq!(symbol_name(&result, retorque.name.name), "retorque");
    assert_eq!(
        symbol_name(&result, genera.params[0].name.name),
        "argumenta"
    );
}

#[test]
fn solum_keyword_usable_as_binding_name() {
    // No reserved words: `solum` (test annotation) is an ordinary identifier
    // outside its annotation slot.
    let result = parse_ok(r"sit _ solum ← 1");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let BindingPattern::Ident(binding) = &var.binding else {
        panic!("expected identifier binding");
    };
    assert_eq!(symbol_name(&result, binding.name), "solum");
}

#[test]
fn mori_keyword_usable_as_binding_name() {
    // No reserved words: `mori` (panic statement) is an ordinary identifier
    // outside its statement slot.
    let result = parse_ok(r"sit _ mori ← 1");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let BindingPattern::Ident(binding) = &var.binding else {
        panic!("expected identifier binding");
    };
    assert_eq!(symbol_name(&result, binding.name), "mori");
}

#[test]
fn parses_test_annotation_keywords_as_import_binding_names() {
    let result = parse_ok(r#"importa ex "norma:solum" solum"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import");
    };
    let ImportKind::Named { name, alias, .. } = &import.kind else {
        panic!("expected named import");
    };
    assert_eq!(symbol_name(&result, name.name), "solum");
    assert!(alias.is_none());
}

#[test]
fn parses_tensor_bridge_member_method_names() {
    let result = parse_ok(
        r"incipit {
  sit tensor<f32, []> t ← vacua
  sit _ g ← t.strue(flat, shape)
  sit _ f ← g.planata()
}",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(entry) = &program.statements[0].kind else {
        panic!("expected incipit entry");
    };
    let radix_syntax::IfBody::Block(block) = &entry.body else {
        panic!("expected incipit block body");
    };
    let StmtKind::Var(structa_call) = &block.statements[1].kind else {
        panic!("expected structa binding");
    };
    let ExprKind::Call(structa_expr) = &structa_call.init.as_ref().expect("init").kind else {
        panic!("expected call");
    };
    let ExprKind::Member(structa_member) = &structa_expr.callee.kind else {
        panic!("expected member callee");
    };
    assert_eq!(symbol_name(&result, structa_member.member.name), "strue");
    let StmtKind::Var(planata_call) = &block.statements[2].kind else {
        panic!("expected planata binding");
    };
    let ExprKind::Call(planata_expr) = &planata_call.init.as_ref().expect("init").kind else {
        panic!("expected call");
    };
    let ExprKind::Member(planata_member) = &planata_expr.callee.kind else {
        panic!("expected member callee");
    };
    assert_eq!(symbol_name(&result, planata_member.member.name), "planata");
}

#[test]
fn rejects_prae_typus_in_param_lists() {
    let source = "functio old(prae typus T, T x) → T {}";
    let result = parse_program(source);
    assert!(!result.success());
    assert_parse_error_has_no_legacy_message(result.errors.first().expect("parse diagnostic"));
}

#[test]
fn parses_function_and_declaration_generic_params() {
    let result = parse_ok(
        r"
functio matmul<T, magnitudo N, magnitudo K>(lista<T> a) → vacuum {}
typus Vector<T, magnitudo N> = lista<T>
",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    assert_eq!(func.type_params.len(), 3);
    assert_eq!(
        func.type_params[0].kind,
        radix_syntax::GenericParamKind::Typus
    );
    assert_eq!(
        func.type_params[1].kind,
        radix_syntax::GenericParamKind::Magnitudo
    );
    let StmtKind::TypeAlias(alias) = &program.statements[1].kind else {
        panic!("expected type alias");
    };
    assert_eq!(alias.type_params.len(), 2);
}

#[test]
fn rejects_type_params_after_magnitudo_params() {
    let source = "functio invalid<magnitudo N, T>(lista<T> values) → vacuum {}";
    assert_parse_error_issue(source, "type_param_after_magnitudo");

    let result = parse_program(source);
    let err = result
        .errors
        .iter()
        .find(|err| {
            err.kind == ParseErrorKind::TypeParameterInvalid
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "type_param_after_magnitudo")
        })
        .expect("type parameter ordering diagnostic");
    assert_parse_error_has_no_legacy_message(err);
    assert_eq!(err.args[1].name, "size_param");
    assert_eq!(err.args[1].value, "magnitudo");
}

#[test]
fn parses_generic_type_default() {
    let result = parse_ok("functio identity<T = textus>(T value) → T { redde value }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    assert_eq!(func.type_params.len(), 1);
    let param = &func.type_params[0];
    assert_eq!(param.kind, radix_syntax::GenericParamKind::Typus);
    let Some(radix_syntax::GenericParamDefault::Typus(ty)) = &param.default else {
        panic!("expected typus default, got {:?}", param.default);
    };
    let TypeExprKind::Named(name, _) = &ty.kind else {
        panic!("expected named type default, got {:?}", ty.kind);
    };
    assert_eq!(symbol_name(&result, name.name), "textus");
}

#[test]
fn parses_generic_size_default() {
    let result = parse_ok("functio bounded<magnitudo D = 64>(numerus x) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    assert_eq!(func.type_params.len(), 1);
    let param = &func.type_params[0];
    assert_eq!(param.kind, radix_syntax::GenericParamKind::Magnitudo);
    let Some(radix_syntax::GenericParamDefault::Magnitudo { value, .. }) = &param.default else {
        panic!("expected magnitudo default, got {:?}", param.default);
    };
    assert_eq!(*value, 64);
}

#[test]
fn parses_trailing_generic_defaults_across_sorts() {
    let result = parse_ok("functio pair<T = textus, magnitudo D = 64>() → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    assert!(func.type_params[0].default.is_some());
    assert!(func.type_params[1].default.is_some());
}

#[test]
fn rejects_non_trailing_generic_default() {
    assert_parse_error_issue(
        "functio f<T = textus, U>(numerus x) → vacuum {}",
        "generic_default_not_trailing",
    );

    let result = parse_program("functio f<T = textus, U>(numerus x) → vacuum {}");
    let err = result
        .errors
        .iter()
        .find(|err| has_arg(err, "issue", "generic_default_not_trailing"))
        .expect("non-trailing default diagnostic");
    assert_eq!(err.kind, ParseErrorKind::TypeParameterInvalid);
    assert_eq!(err.args[1].name, "parameter");
    assert_eq!(err.args[1].value, "U");
}

#[test]
fn rejects_generic_default_missing_value() {
    assert_parse_error_issue(
        "functio f<T = >(numerus x) → vacuum {}",
        "generic_default_missing_value",
    );
    assert_parse_error_issue(
        "functio f<magnitudo D = >(numerus x) → vacuum {}",
        "generic_default_missing_value",
    );
}

#[test]
fn rejects_generic_default_wrong_expression_class() {
    // A magnitudo default is a NATURAL literal, never a name or expression.
    assert_parse_error_issue(
        "functio f<magnitudo D = D>(numerus x) → vacuum {}",
        "generic_default_wrong_class",
    );
    // A typus default is a type annotation, never a natural literal.
    assert_parse_error_issue(
        "functio f<T = 64>(numerus x) → vacuum {}",
        "generic_default_wrong_class",
    );
}

#[test]
fn rejects_vel_generic_default_spelling() {
    assert_parse_error_issue(
        "functio f<T vel 64>(numerus x) → vacuum {}",
        "removed_vel_generic_default",
    );
    // `coalesce` is the en-surface spelling of `vel` and hits the same rule.
    assert_en_parse_error_issue(
        "fn f<size D coalesce 64>() → void {}",
        "removed_vel_generic_default",
    );
}

// =============================================================================
// mori / iace optional si guard
// =============================================================================

#[test]
fn mori_without_si_is_unconditional() {
    let result = parse_ok(r#"incipit { mori "boom" }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(body) = &incipit.body else {
        panic!("expected incipit block body");
    };
    assert!(
        matches!(body.statements[0].kind, StmtKind::Mori(_)),
        "mori without si should be unconditional Mori"
    );
}

#[test]
fn mori_with_si_desugars_to_si_block() {
    let result = parse_ok(r#"incipit { mori "boom" si x ≺ 0 }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(body) = &incipit.body else {
        panic!("expected incipit block body");
    };
    let StmtKind::Si(si) = &body.statements[0].kind else {
        panic!("expected Si, got {:?}", body.statements[0].kind);
    };
    let IfBody::Block(block) = &si.then else {
        panic!("expected block body");
    };
    assert!(
        matches!(block.statements[0].kind, StmtKind::Mori(_)),
        "inner statement should be Mori"
    );
}

#[test]
fn iace_without_si_is_unconditional() {
    let result = parse_ok(r#"incipit { iace "err" }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(body) = &incipit.body else {
        panic!("expected incipit block body");
    };
    assert!(
        matches!(body.statements[0].kind, StmtKind::Iace(_)),
        "iace without si should be unconditional Iace"
    );
}

#[test]
fn iace_with_si_desugars_to_si_block() {
    let result = parse_ok(r#"incipit { iace "err" si ok }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(body) = &incipit.body else {
        panic!("expected incipit block body");
    };
    let StmtKind::Si(si) = &body.statements[0].kind else {
        panic!("expected Si, got {:?}", body.statements[0].kind);
    };
    let IfBody::Block(block) = &si.then else {
        panic!("expected block body");
    };
    assert!(
        matches!(block.statements[0].kind, StmtKind::Iace(_)),
        "inner statement should be Iace"
    );
}

#[test]
fn mori_si_inside_function_body() {
    let result = parse_ok(
        r#"functio f(numerus x) → vacuum {
  mori "bad" si x ≺ 0
  redde
}"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("function body");
    let StmtKind::Si(si) = &body.statements[0].kind else {
        panic!(
            "expected Si inside function, got {:?}",
            body.statements[0].kind
        );
    };
    assert!(si.catch.is_none());
    assert!(si.else_.is_none());
}

#[test]
fn mori_si_with_gt_comparison() {
    let result = parse_ok(r#"incipit { mori "bad" si a ≻ b }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(body) = &incipit.body else {
        panic!("expected incipit block body");
    };
    assert!(
        matches!(body.statements[0].kind, StmtKind::Si(_)),
        "mori si with ≻ should desugar to Si"
    );
}

#[test]
fn mori_with_si_on_next_line_stays_unconditional() {
    // When si is on the next line, it is a new statement, not a guard.
    let result = parse_ok(
        r#"incipit {
  mori "boom"
  si ok ergo tacet
}"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(body) = &incipit.body else {
        panic!("expected incipit block body");
    };
    // First statement should be unconditional mori
    assert!(
        matches!(body.statements[0].kind, StmtKind::Mori(_)),
        "mori should be unconditional when si is on next line"
    );
    // Second statement should be the si
    assert!(
        matches!(body.statements[1].kind, StmtKind::Si(_)),
        "next-line si should be its own statement"
    );
}

#[test]
fn mori_si_inside_ergo_body() {
    let result = parse_ok(
        r#"functio f(bivalens ok) → vacuum {
  si ok ergo mori "bad" si ok
}"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("function body");
    let StmtKind::Si(outer_si) = &body.statements[0].kind else {
        panic!("expected outer Si");
    };
    // The ergo body should be a Si wrapping a Mori
    let IfBody::Ergo(ergo_stmt) = &outer_si.then else {
        panic!("expected ergo body");
    };
    let StmtKind::Si(_inner_si) = &ergo_stmt.kind else {
        panic!(
            "expected inner Si from mori si guard, got {:?}",
            ergo_stmt.kind
        );
    };
}

#[test]
fn mori_si_with_complex_expression() {
    let result = parse_ok(r#"incipit { mori "bad" si x ≺ 0 aut x ≻ 100 }"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(incipit) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(body) = &incipit.body else {
        panic!("expected incipit block body");
    };
    assert!(
        matches!(body.statements[0].kind, StmtKind::Si(_)),
        "mori si with aut expression should desugar to Si"
    );
}

#[test]
fn mori_si_does_not_consume_next_si_statement() {
    // Regression: mori si guard should not eat the next statement's si keyword.
    let result = parse_ok(
        r#"custodi {
  si n ≡ 0 ergo mori "first" si n ≺ 0
  si n ≡ 1 ergo mori "second"
}"#,
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Custodi(custodi) = &program.statements[0].kind else {
        panic!("expected custodi");
    };
    assert_eq!(custodi.clauses.len(), 2, "should have two custodi guards");
    // Second guard should still parse — its si was not consumed by the first guard
    assert!(
        matches!(custodi.clauses[1].body, IfBody::Ergo(_)),
        "second guard should have ergo body"
    );
}

// =============================================================================
// Morphology-driven async/stream surface
// =============================================================================

fn func_posture(result: &super::ParseResult) -> CallablePosture {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function declaration");
    };
    func.posture
}

#[test]
fn parses_fiet_posture() {
    let result = parse_ok("functio f() fiet → vacuum { tacet }");
    assert_eq!(func_posture(&result), CallablePosture::AsyncFinite);
}

#[test]
fn parses_fiunt_posture() {
    let result = parse_ok("functio f() fiunt → numerus { cede 1 }");
    assert_eq!(func_posture(&result), CallablePosture::SyncStream);
}

#[test]
fn parses_fient_posture() {
    let result = parse_ok("functio f() fient → numerus { cede 1 }");
    assert_eq!(func_posture(&result), CallablePosture::AsyncStream);
}

#[test]
fn parses_cede_as_yield_statement_only() {
    let result = parse_ok("functio f() fiunt → numerus { cede 1 }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Expr(expr_stmt) = &body.statements[0].kind else {
        panic!("expected expression statement");
    };
    assert!(matches!(expr_stmt.expr.kind, ExprKind::Cede(_)));
}

#[test]
fn cede_keyword_usable_as_identifier_in_initializer() {
    // No reserved words: `cede` (yield) is an ordinary identifier outside its
    // generator-yield slot, so it is a valid initializer expression.
    let result = parse_ok("functio f() fiunt → numerus { sit _ x ← cede }");
    assert_eq!(
        result.program.as_ref().expect("program").statements.len(),
        1
    );
}

#[test]
fn cede_keyword_usable_as_identifier_in_call_argument() {
    // No reserved words: `cede` is an ordinary identifier in call-argument
    // position (its generator-yield claim is statement-shaped).
    let result = parse_ok("functio f() fiunt → numerus { nota cede }");
    assert_eq!(
        result.program.as_ref().expect("program").statements.len(),
        1
    );
}

#[test]
fn parses_bare_posture_is_sync_finite() {
    let result = parse_ok("functio f() → vacuum { tacet }");
    assert_eq!(func_posture(&result), CallablePosture::SyncFinite);
}

#[test]
fn parses_figendum_await_binding() {
    let result = parse_ok("functio f() fiet → vacuum { figendum numerus x ← fut redde x }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Var(decl) = &body.statements[0].kind else {
        panic!("expected var decl");
    };
    assert!(decl.is_await());
    assert_eq!(decl.await_form, Some(AwaitBindingForm::Morphology));
    assert_eq!(decl.mutability, AstMutability::Immutable);
}

#[test]
fn parses_variandum_await_binding() {
    let result = parse_ok("functio f() fiet → vacuum { variandum numerus x ← fut redde x }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Var(decl) = &body.statements[0].kind else {
        panic!("expected var decl");
    };
    assert!(decl.is_await());
    assert_eq!(decl.await_form, Some(AwaitBindingForm::Morphology));
    assert_eq!(decl.mutability, AstMutability::Mutable);
}

#[test]
fn parses_await_directed_fixum_binding() {
    let result = parse_ok("functio f() fiet → vacuum { sit numerus x ↢ fut redde x }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Var(decl) = &body.statements[0].kind else {
        panic!("expected var decl");
    };
    assert!(decl.is_await());
    assert_eq!(decl.await_form, Some(AwaitBindingForm::Assignment));
    assert_eq!(decl.mutability, AstMutability::Immutable);
}

#[test]
fn parses_await_directed_varia_and_sit_bindings() {
    let result =
        parse_ok("functio f() fiet → vacuum { varia numerus x ↢ fut sit _ y ↢ fut redde y }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Var(varia) = &body.statements[0].kind else {
        panic!("expected varia declaration");
    };
    let StmtKind::Var(sit) = &body.statements[1].kind else {
        panic!("expected sit declaration");
    };
    assert!(varia.is_await());
    assert_eq!(varia.await_form, Some(AwaitBindingForm::Assignment));
    assert_eq!(varia.mutability, AstMutability::Mutable);
    assert!(sit.is_await());
    assert_eq!(sit.await_form, Some(AwaitBindingForm::Assignment));
    assert_eq!(sit.mutability, AstMutability::Immutable);
}

#[test]
fn await_directed_assignment_is_not_a_general_expression_operator() {
    let result = parse_program("functio f() fiet → vacuum { x ↢ fut }");
    assert!(!result.success());
}

#[test]
fn rejects_await_directed_destructuring_and_missing_initializers() {
    for source in [
        "functio f() fiet { fixum [x] ↢ future }",
        "functio f() fiet { sit numerus x ↢ }",
    ] {
        let result = parse_program(source);
        assert!(
            !result.errors.is_empty(),
            "expected parse failure for {source:?}"
        );
    }
}

#[test]
fn parses_reddet_await_return() {
    let result = parse_ok("functio f() fiet → vacuum { reddet fut }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    assert!(matches!(body.statements[0].kind, StmtKind::Reddet(_)));
}

#[test]
fn parses_tacebit_await_discard() {
    let result = parse_ok("functio f() fiet → vacuum { tacebit fut }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    assert!(matches!(body.statements[0].kind, StmtKind::Tacebit(_)));
}

#[test]
fn posture_words_remain_legal_identifiers() {
    // fiet/fiunt/fient/figendum/variandum/reddet/tacebit must remain usable as
    // ordinary identifiers outside their grammar slots.
    let result = parse_ok("functio f() → vacuum { sit numerus fiet ← 1 redde fiet }");
    assert!(result.success());
}

#[test]
fn parses_posture_before_error_channel() {
    let result = parse_ok("functio f() fiet → vacuum ⇥ textus { tacet }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    assert_eq!(func.posture, CallablePosture::AsyncFinite);
    assert!(func.err.is_some());
}

#[test]
fn parses_posture_in_genus_method() {
    let result = parse_ok("genus C { functio m() fiet → vacuum { tacet } }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(cls) = &program.statements[0].kind else {
        panic!("expected class");
    };
    let ClassMemberKind::Method(method) = &cls.members[0].kind else {
        panic!("expected method");
    };
    assert_eq!(method.posture, CallablePosture::AsyncFinite);
}

#[test]
fn parses_posture_in_implendum_method_signature() {
    let result = parse_ok("implendum C { functio m() fient → numerus ⇥ textus }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Interface(interface) = &program.statements[0].kind else {
        panic!("expected interface");
    };
    assert_eq!(interface.methods[0].posture, CallablePosture::AsyncStream);
    assert!(interface.methods[0].ret.is_some());
    assert!(interface.methods[0].err.is_some());
}

// =============================================================================
// CDA-U2 — ↤ (U+21A4) conversion-directed assignment: red decl-parser tests
// =============================================================================
// Typed `fixum`/`varia` declarations accept `↤` initializers (goal decision 6).
// Today every fixture FAILS at the lexer with `unexpected_character` for the
// U+21A4 glyph — the expected missing-feature reason. U3 lands the token and
// the initializer grammar; the untargeted-form rejections (`fixum _`, `sit`,
// destructuring, awaited) are asserted in the semantic cluster.

#[test]
fn conversio_assign_typed_fixum_and_varia_initializers_parse() {
    let result = parse_ok("sit numerus n ↤ \"42\"\nvaria numerus count ↤ raw ⊥ 0");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);
    for stmt in &program.statements {
        let StmtKind::Var(var) = &stmt.kind else {
            panic!("expected variable declaration");
        };
        let expr = var.init.as_deref().expect("initializer");
        assert!(
            matches!(expr.kind, ExprKind::Assign(_)),
            "↤ initializer must parse as an assignment expression, got {:?}",
            expr.kind
        );
    }
}

// =============================================================================
// CDA-U3 — ↤ (U+21A4): typed-initializer acceptance and structured
// decl-level rejections
// =============================================================================
// The typed `fixum`/`varia` initializer grammar landed in U3; the untargeted
// forms reject at the parser with stable issue identity (renderer-owned prose).
// These tags match U2's forward-looking semantic expectations so the same
// issue flows through the compile pipeline.

#[test]
fn conversio_assign_untyped_fixum_initializer_rejects_with_concrete_type_issue() {
    // `fixum _` has no written type → no concrete conversion target.
    assert_parse_error_issue(
        "incipit {\n  sit _ answer ↤ \"42\"\n}",
        "conversio_assign_requires_concrete_type",
    );
}

#[test]
fn conversio_assign_sit_initializer_rejects_with_concrete_type_issue() {
    // `sit` is an inferred binding (≡ `fixum _`) → no concrete target.
    assert_parse_error_issue(
        "incipit {\n  sit _ answer ↤ \"42\"\n}",
        "conversio_assign_requires_concrete_type",
    );
}

#[test]
fn conversio_assign_untyped_destructuring_initializer_rejects_with_concrete_type_issue() {
    // Untyped destructuring patterns have no concrete conversion target.
    assert_parse_error_issue(
        "incipit {\n  fixum { field } ↤ \"x\"\n}",
        "conversio_assign_requires_concrete_type",
    );
}

#[test]
fn conversio_assign_awaited_binding_initializer_rejects_with_await_issue() {
    // `figendum`/`variandum` retain `←` (goal decision 6): implicit
    // conversion plus await ordering needs a separate language decision.
    for source in [
        "incipit {\n  figendum numerus n ↤ \"42\"\n}",
        "incipit {\n  variandum numerus n ↤ \"42\"\n}",
    ] {
        assert_parse_error_issue(source, "conversio_assign_await_unsupported");
    }
}

// =============================================================================
// EOT-1 — additive parser red drivers and legacy baseline
// =============================================================================

const EOT_SELECTIVE_IMPORT_ISSUES: &[&str] = &[
    "selective_import_binding_expected",
    "selective_import_missing_member",
    "selective_import_duplicate_alias",
];

const EOT_TRANSFER_ISSUES: &[&str] = &[
    "transfer_sink_retired",
    "copy_into_target_not_tensor",
    "copy_into_type_mismatch",
];

#[test]
fn eot_selective_import_is_currently_rejected_at_the_expression_boundary() {
    // EOT-2S: the selective form now parses as one declaration instead of
    // failing at the expression boundary; this is the EOT-1 driver flip.
    let source = r#"importa ex "norma:consolum" fixum dic ut output, funde ut output_bytes,"#;
    let result = parse_program(source);
    assert!(
        result.success(),
        "selective import must parse as one declaration; errors={:?}",
        result.errors
    );
    let program = result.program.expect("selective import program");
    assert_eq!(program.statements.len(), 1, "one selective declaration");
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };
    assert!(
        matches!(import.kind, ImportKind::Selective { .. }),
        "expected selective import kind, got {:?}",
        import.kind
    );
}

#[test]
fn eot_selective_import_acceptance_red_driver() {
    let source = r#"importa ex "norma:consolum" fixum dic ut output, funde ut output_bytes,"#;
    let result = parse_program(source);
    assert!(
        result.success(),
        "EOT-2S red driver: selective imports must parse as one declaration; current errors={:?}; later issue names={EOT_SELECTIVE_IMPORT_ISSUES:?}",
        result.errors
    );
}

#[test]
fn eot_transfer_expression_acceptance_red_driver() {
    let result = parse_program("incipit { output ⇇ \"salve\\n\" }");
    assert!(
        result.success(),
        "EOT-3S red driver: glyph-only transfer must parse; current errors={:?}; later issue names={EOT_TRANSFER_ISSUES:?}",
        result.errors
    );
}

#[test]
fn eot_legacy_import_output_and_input_forms_remain_accepted() {
    for source in [
        r#"importa ex "norma:consolum" consolum"#,
        "nota \"legacy text\"",
        "mone \"legacy warning\"",
        "vide \"legacy debug\"",
        "scribe \"legacy compatibility\"",
        "incipit { sit _ input ← lege nota input }",
    ] {
        let result = parse_program(source);
        assert!(
            result.success(),
            "legacy EOT baseline must remain accepted for {source:?}: {:?}",
            result.errors
        );
    }
}

// =============================================================================
// EOT-2S — selective-import syntax and AST
// =============================================================================

#[test]
fn eot_selective_import_preserves_member_order_and_exact_spans() {
    let source = r#"importa ex "norma:consolum" fixum dic ut output, funde ut output_bytes,"#;
    let result = parse_ok(source);
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1, "one selective declaration");

    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };
    assert_eq!(symbol_name(&result, import.path), "norma:consolum");
    let ImportKind::Selective { members } = &import.kind else {
        panic!("expected selective import kind, got {:?}", import.kind);
    };
    assert_eq!(members.len(), 2, "both selective members in source order");

    assert_eq!(symbol_name(&result, members[0].name.name), "dic");
    assert_eq!(
        symbol_name(&result, members[0].alias.as_ref().expect("alias").name),
        "output"
    );
    assert_eq!(symbol_name(&result, members[1].name.name), "funde");
    assert_eq!(
        symbol_name(&result, members[1].alias.as_ref().expect("alias").name),
        "output_bytes"
    );

    // Exact source spans for every member name and alias.
    assert_eq!(members[0].name.span, span_for(source, "dic"));
    assert_eq!(
        members[0].alias.as_ref().expect("alias").span,
        span_for(source, "output")
    );
    assert_eq!(members[1].name.span, span_for(source, "funde"));
    assert_eq!(
        members[1].alias.as_ref().expect("alias").span,
        span_for(source, "output_bytes")
    );
    assert_eq!(
        members[0].span,
        span_for(source, "dic").merge(span_for(source, "output"))
    );
}

#[test]
fn eot_selective_import_mixed_aliased_and_unaliased_members() {
    let result = parse_ok(r#"importa ex "norma:consolum" fixum dic, funde ut output_bytes,"#);
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };
    let ImportKind::Selective { members } = &import.kind else {
        panic!("expected selective import kind, got {:?}", import.kind);
    };
    assert_eq!(members.len(), 2);
    assert_eq!(symbol_name(&result, members[0].name.name), "dic");
    assert!(
        members[0].alias.is_none(),
        "unaliased member stays unaliased"
    );
    assert_eq!(symbol_name(&result, members[1].name.name), "funde");
    assert_eq!(
        symbol_name(&result, members[1].alias.as_ref().expect("alias").name),
        "output_bytes"
    );
}

#[test]
fn eot_selective_import_trailing_comma_without_following_member() {
    let result = parse_ok(r#"importa ex "norma:consolum" fixum dic ut output,"#);
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1);
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };
    let ImportKind::Selective { members } = &import.kind else {
        panic!("expected selective import kind, got {:?}", import.kind);
    };
    assert_eq!(members.len(), 1);
    assert_eq!(symbol_name(&result, members[0].name.name), "dic");
}

#[test]
fn eot_selective_import_does_not_consume_following_declaration() {
    let result = parse_ok(
        r#"importa ex "norma:consolum" fixum dic ut output, funde ut output_bytes,
nota "importa-selective"
"#,
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(
        program.statements.len(),
        2,
        "import + following declaration"
    );

    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };
    let ImportKind::Selective { members } = &import.kind else {
        panic!("expected selective import kind, got {:?}", import.kind);
    };
    assert_eq!(members.len(), 2);
    assert_eq!(symbol_name(&result, members[0].name.name), "dic");
    assert_eq!(symbol_name(&result, members[1].name.name), "funde");
    assert!(
        matches!(program.statements[1].kind, StmtKind::Scribe(_)),
        "following declaration must not be consumed as a member"
    );
}

#[test]
fn eot_selective_import_visibility_marker_preserved() {
    let result = parse_ok(r#"importa ex "norma:consolum" publica fixum dic ut output,"#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Import(import) = &program.statements[0].kind else {
        panic!("expected import statement");
    };
    assert_eq!(import.visibility, radix_syntax::Visibility::Publica);
    let ImportKind::Selective { members } = &import.kind else {
        panic!("expected selective import kind, got {:?}", import.kind);
    };
    assert_eq!(members.len(), 1);
    assert_eq!(symbol_name(&result, members[0].name.name), "dic");
}

#[test]
fn eot_selective_import_empty_list_rejected() {
    assert_parse_error_matches(
        r#"importa ex "norma:consolum" fixum"#,
        ParseExpectation::Issue("selective_import_missing_member"),
    );
    assert_parse_error_matches(
        r#"importa ex "norma:consolum" fixum ,"#,
        ParseExpectation::Issue("selective_import_missing_member"),
    );
}

#[test]
fn eot_selective_import_malformed_alias_rejected() {
    assert_parse_error_matches(
        r#"importa ex "norma:consolum" fixum dic ut"#,
        ParseExpectation::Issue("selective_import_binding_expected"),
    );
    assert_parse_error_matches(
        r#"importa ex "norma:consolum" fixum dic ut,"#,
        ParseExpectation::Issue("selective_import_binding_expected"),
    );
    assert_parse_error_matches(
        r#"importa ex "norma:consolum" fixum ut output"#,
        ParseExpectation::Issue("selective_import_binding_expected"),
    );
}

#[test]
fn eot_selective_import_wildcard_mixing_rejected() {
    assert_parse_error_matches(
        r#"importa ex "norma:consolum" fixum * ut all"#,
        ParseExpectation::Issue("selective_import_wildcard_mixing"),
    );
    assert_parse_error_matches(
        r#"importa ex "norma:consolum" fixum dic, * ut all"#,
        ParseExpectation::Issue("selective_import_wildcard_mixing"),
    );
    assert_parse_error_matches(
        r#"importa ex "norma:consolum" fixum dic * ut all"#,
        ParseExpectation::Issue("selective_import_wildcard_mixing"),
    );
}

#[test]
fn eot_selective_import_missing_separator_rejected() {
    // An unaliased first member (`fixum dic funde`) is genuinely ambiguous with
    // a following typed `fixum` declaration, so it keeps the following-statement
    // reading. After an aliased member the list is unambiguous and a bare name
    // next to it reports the missing separator.
    assert_parse_error_matches(
        r#"importa ex "norma:consolum" fixum dic ut output funde"#,
        ParseExpectation::Issue("item_list_missing_separator"),
    );
}

// =============================================================================
// EOT-3S — transfer syntax, AST, and chain precedence
// =============================================================================

/// Extract the single top-level expression statement's expression.
fn eot_transfer_expr(result: &ParseResult) -> &radix_syntax::Expr {
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 1, "one expression statement");
    let StmtKind::Expr(expr_stmt) = &program.statements[0].kind else {
        panic!(
            "expected expression statement, got {:?}",
            program.statements[0].kind
        );
    };
    &expr_stmt.expr
}

#[test]
fn copy_into_one_target_one_value() {
    let result = parse_ok(r#"output ⇇ "salve\n""#);
    let expr = eot_transfer_expr(&result);
    let ExprKind::CopyInto(copy) = &expr.kind else {
        panic!("expected copy-into expression, got {:?}", expr.kind);
    };
    let ExprKind::Ident(target) = &copy.target.kind else {
        panic!("expected ident target, got {:?}", copy.target.kind);
    };
    assert_eq!(symbol_name(&result, target.name), "output");
    assert!(
        matches!(copy.value.kind, ExprKind::Literal(Literal::String(_))),
        "value is the string literal, got {:?}",
        copy.value.kind
    );
}

#[test]
fn eot_transfer_chain_is_rejected_with_a_named_issue() {
    // `⇇` is a single copy-into statement: a chain is a parse error, not a
    // flat list of payloads.
    assert_parse_error_matches(
        r#"output ⇇ "a" ⇇ "b""#,
        ParseExpectation::Issue("copy_into_chain"),
    );
    assert_parse_error_matches(
        r#"output ⇇ "a" ⇇ "b" ⇇ "c""#,
        ParseExpectation::Issue("copy_into_chain"),
    );
}

#[test]
fn eot_transfer_groups_around_calls_conversions_and_templates() {
    // The target is a complete postfix call; the value is a complete
    // conversion. The ⇇ sits outside both.
    let result = parse_ok(r#"scribe("x") ⇇ payload ↦ textus"#);
    let expr = eot_transfer_expr(&result);
    let ExprKind::CopyInto(copy) = &expr.kind else {
        panic!("expected copy-into expression, got {:?}", expr.kind);
    };
    assert!(
        matches!(copy.target.kind, ExprKind::Call(_)),
        "target keeps the complete call, got {:?}",
        copy.target.kind
    );
    assert!(
        matches!(copy.value.kind, ExprKind::Conversio(_)),
        "value is the complete conversion, got {:?}",
        copy.value.kind
    );

    let result = parse_ok(r#"output ⇇ "§ §\n"(a, b)"#);
    let expr = eot_transfer_expr(&result);
    let ExprKind::CopyInto(copy) = &expr.kind else {
        panic!("expected copy-into expression, got {:?}", expr.kind);
    };
    let ExprKind::Call(template) = &copy.value.kind else {
        panic!(
            "value is the string-constructor application, got {:?}",
            copy.value.kind
        );
    };
    assert!(
        matches!(template.callee.kind, ExprKind::Literal(Literal::String(_))),
        "template callee is the string literal"
    );
    assert_eq!(template.args.len(), 2);
}

#[test]
fn eot_transfer_binary_operators_group_into_the_value() {
    // ⇇ is locked at low precedence: every binary operator binds into the
    // value, never around the statement.
    let result = parse_ok(r"output ⇇ (q · kt) ⊙ s");
    let expr = eot_transfer_expr(&result);
    let ExprKind::CopyInto(copy) = &expr.kind else {
        panic!("expected copy-into expression, got {:?}", expr.kind);
    };
    assert!(
        matches!(copy.value.kind, ExprKind::Binary(_)),
        "value keeps `(q · kt) ⊙ s`, got {:?}",
        copy.value.kind
    );
}

#[test]
fn eot_transfer_inside_assignment_value() {
    // `⇇` binds tighter than `←`: the copy-into is the assigned value. The
    // parser admits the shape; the semantic layer rejects it as a result used
    // as a value (`copy_into_value_used`).
    let result = parse_ok(r#"sit _ sink ← output ⇇ "a""#);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let init = var.init.as_deref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::CopyInto(_)),
        "declaration initializer is the copy-into, got {:?}",
        init.kind
    );
}

#[test]
fn eot_transfer_missing_payload_rejected() {
    assert_parse_error_matches(
        r"output ⇇",
        ParseExpectation::Issue("copy_into_value_expected"),
    );
    assert_parse_error_matches(
        r#"output ⇇ "a" ⇇"#,
        ParseExpectation::Issue("copy_into_chain"),
    );
}

// =============================================================================
// 0c4d3dc1 — `s ⇜ x` sink-into syntax
// =============================================================================

#[test]
fn sink_into_statement_parses() {
    let result = parse_ok("output \u{21dc} \"salve\\n\"");
    let ExprKind::SinkInto(sink) = &eot_transfer_expr(&result).kind else {
        panic!(
            "expected sink-into expression, got {:?}",
            eot_transfer_expr(&result).kind
        );
    };
    let ExprKind::Ident(sink_name) = &sink.sink.kind else {
        panic!("expected ident sink, got {:?}", sink.sink.kind);
    };
    assert_eq!(symbol_name(&result, sink_name.name), "output");
    assert!(
        matches!(sink.item.kind, ExprKind::Literal(Literal::String(_))),
        "item is the string literal, got {:?}",
        sink.item.kind
    );
}

#[test]
fn sink_into_chain_rejected() {
    // One `⇜` is one write: neither a second `⇜` nor a following `⇇` is a
    // chain of results.
    assert_parse_error_matches(
        r#"output ⇜ "a" ⇜ "b""#,
        ParseExpectation::Issue("sink_into_chain"),
    );
    assert_parse_error_matches(
        r#"output ⇜ "a" ⇇ "b""#,
        ParseExpectation::Issue("sink_into_chain"),
    );
}

#[test]
fn sink_into_item_expected() {
    assert_parse_error_matches(
        r"output ⇜",
        ParseExpectation::Issue("sink_into_item_expected"),
    );
}

#[test]
fn sink_into_copy_into_mix_rejected() {
    // `a ⇇ b ⇜ c` reaches the sink-into suffix with a copy-into already
    // parsed: that is a mixed chain, not a nested statement.
    assert_parse_error_matches(
        r#"output ⇇ "a" ⇜ "b""#,
        ParseExpectation::Issue("sink_into_chain"),
    );
}

struct EnSizePack;

impl KeywordPack for EnSizePack {
    fn pack_id(&self) -> &'static str {
        "en-size-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<radix_lexer::TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "size" => Some("magnitudo"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "mut" => Some("varia"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

#[test]
fn const_size_declaration_parses_and_bare_or_var_forms_are_rejected() {
    for source in ["{ const size H ← 3 }", "{ fixum magnitudo H ← 3 }"] {
        let result = if source.contains(" size ") {
            parse(lex_with_locale_pack(source, &EnSizePack))
        } else {
            parse_ok(source)
        };
        assert!(result.success(), "{source}: {:?}", result.errors);
        let program = result.program.as_ref().expect("program");
        let StmtKind::Block(body) = &program.statements[0].kind else {
            panic!("expected block");
        };
        let StmtKind::SizeDecl(decl) = &body.statements[0].kind else {
            panic!("expected size declaration");
        };
        assert_eq!(symbol_name(&result, decl.name.name), "H");
        assert!(!decl.is_static);
    }

    for (source, issue) in [
        ("{ size H ← 3 }", "size_requires_const"),
        ("{ mut size H ← 3 }", "size_is_write_once"),
    ] {
        let result = parse(lex_with_locale_pack(source, &EnSizePack));
        assert!(
            result.errors.iter().any(|error| error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)),
            "expected {issue} for {source}, got {:?}",
            result.errors
        );
        if issue == "size_requires_const" {
            assert!(
                result.errors[0]
                    .args
                    .iter()
                    .any(|arg| arg.name == "suggestion" && arg.value == "const size H")
            );
        }
    }

    let static_result = parse(lex_with_locale_pack("{ const size N = 3 }", &EnSizePack));
    assert!(
        static_result.success(),
        "static size: {:?}",
        static_result.errors
    );
    let StmtKind::Block(body) = &static_result.program.unwrap().statements[0].kind else {
        panic!("expected block");
    };
    let StmtKind::SizeDecl(decl) = &body.statements[0].kind else {
        panic!("expected size declaration");
    };
    assert!(decl.is_static);
    let nonliteral = parse(lex_with_locale_pack(
        "{ const size N = other }",
        &EnSizePack,
    ));
    assert!(nonliteral.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.value == "size_static_requires_literal")
    }));
    let lexed = lex_with_locale_pack("size", &EnSizePack);
    assert!(matches!(lexed.tokens[0].kind, TokenKind::Ident(_)));
}

/// Parses a `tabula` binding with `init` as its initializer and hands the
/// initializer expression kind to `check`.
fn with_inferred_constructor_init(init: &str, check: impl FnOnce(&ExprKind)) {
    with_inferred_constructor_init_result(init, |_, kind| check(kind));
}

/// [`with_inferred_constructor_init`] plus the parse result, so checks can
/// resolve member-key and type-head symbols.
fn with_inferred_constructor_init_result(init: &str, check: impl FnOnce(&ParseResult, &ExprKind)) {
    let result = parse_ok(&format!(
        "incipit {{ sit tabula<textus, numerus> m ← {init} }}"
    ));
    let program = result.program.as_ref().expect("program");
    let StmtKind::Incipit(entry) = &program.statements[0].kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(block) = &entry.body else {
        panic!("expected incipit block");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected binding");
    };
    check(&result, &var.init.as_deref().expect("initializer").kind);
}

#[test]
fn inferred_constructor_entries_parse_as_hole_verte_over_json_object() {
    with_inferred_constructor_init("_ { \"a\": 1, \"b\": 2 }", |kind| {
        let ExprKind::Verte(verte) = kind else {
            panic!("expected Verte");
        };
        assert!(matches!(verte.ty.kind, TypeExprKind::Infer));
        let ExprKind::Literal(Literal::JsonValor(JsonValue::Object(members))) = &verte.expr.kind
        else {
            panic!("entries must be the bare-literal JSON members");
        };
        assert_eq!(members.len(), 2);
    });
}

#[test]
fn inferred_constructor_empty_braces_parse_as_hole_verte_over_empty_object() {
    with_inferred_constructor_init("_ { }", |kind| {
        let ExprKind::Verte(verte) = kind else {
            panic!("expected Verte");
        };
        assert!(matches!(verte.ty.kind, TypeExprKind::Infer));
        assert!(matches!(verte.expr.kind, ExprKind::Object(_)));
    });
}

#[test]
fn inferred_constructor_brackets_parse_as_hole_verte_over_array() {
    with_inferred_constructor_init("_ [1, 2]", |kind| {
        let ExprKind::Verte(verte) = kind else {
            panic!("expected Verte");
        };
        assert!(matches!(verte.ty.kind, TypeExprKind::Infer));
        assert!(matches!(verte.expr.kind, ExprKind::Array(_)));
    });
}

#[test]
fn bare_field_literal_parses_as_hole_verte_over_object() {
    with_inferred_constructor_init("{ x = 1, y = 2 }", |kind| {
        let ExprKind::Verte(verte) = kind else {
            panic!("expected Verte");
        };
        assert!(matches!(verte.ty.kind, TypeExprKind::Infer));
        assert!(matches!(verte.expr.kind, ExprKind::Object(_)));
    });
}

#[test]
fn stated_keyed_constructor_json_parses_as_ascription_of_json_literal() {
    with_inferred_constructor_init_result("json { \"x\": 10 }", |result, kind| {
        let ExprKind::Verte(verte) = kind else {
            panic!("expected Verte");
        };
        let TypeExprKind::Named(head, args) = &verte.ty.kind else {
            panic!("expected named type head");
        };
        assert_eq!(symbol_name(result, head.name), "json");
        assert!(args.is_empty(), "json takes no type arguments");
        let ExprKind::Literal(Literal::JsonValor(JsonValue::Object(members))) = &verte.expr.kind
        else {
            panic!("keyed entries must be the bare-literal JSON members");
        };
        let keys: Vec<&str> = members
            .iter()
            .map(|member| symbol_name(result, member.key))
            .collect();
        assert_eq!(keys, ["x"]);
    });
}

#[test]
fn stated_keyed_constructor_map_parses_with_type_arguments() {
    with_inferred_constructor_init_result("map<string, int> { \"a\": 1 }", |result, kind| {
        let ExprKind::Verte(verte) = kind else {
            panic!("expected Verte");
        };
        let TypeExprKind::Named(head, args) = &verte.ty.kind else {
            panic!("expected named type head");
        };
        assert_eq!(symbol_name(result, head.name), "map");
        assert_eq!(args.len(), 2, "map carries key and value type arguments");
        let arg_names: Vec<&str> = args
            .iter()
            .map(|arg| match &generic_arg_type(arg).kind {
                TypeExprKind::Named(name, _) => symbol_name(result, name.name),
                other => panic!("expected named type argument, got {other:?}"),
            })
            .collect();
        assert_eq!(arg_names, ["string", "int"]);
        let ExprKind::Literal(Literal::JsonValor(JsonValue::Object(members))) = &verte.expr.kind
        else {
            panic!("keyed entries must be the bare-literal JSON members");
        };
        let keys: Vec<&str> = members
            .iter()
            .map(|member| symbol_name(result, member.key))
            .collect();
        assert_eq!(keys, ["a"]);
    });
}

#[test]
fn stated_keyed_constructor_keeps_field_form() {
    with_inferred_constructor_init_result("Point { x = 1 }", |result, kind| {
        let ExprKind::Verte(verte) = kind else {
            panic!("expected Verte");
        };
        let TypeExprKind::Named(head, args) = &verte.ty.kind else {
            panic!("expected named type head");
        };
        assert_eq!(symbol_name(result, head.name), "Point");
        assert!(args.is_empty());
        let ExprKind::Object(object) = &verte.expr.kind else {
            panic!("named field construction must keep the object node");
        };
        let radix_syntax::ObjectKey::Ident(field) = &object.fields[0].key else {
            panic!("expected an identifier field key");
        };
        assert_eq!(symbol_name(result, field.name), "x");
    });
}
