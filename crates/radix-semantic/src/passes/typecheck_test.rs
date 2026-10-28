//! Tests for the typecheck pass.
//!
//! Tests are grouped into three categories:
//! - **Source-based** — tests that compile Faber source via the unified harness
//!   (`test_support`) and assert success, failure, or specific diagnostic issues.
//! - **HIR-inspection** — tests that run the full analysis pipeline and then
//!   inspect the typed HIR for post-typecheck properties (e.g., inferred return
//!   types).
//! - **Internal-checker** — tests that call `TypeChecker` methods directly
//!   because the HIR state under test is unreachable from valid source or the
//!   test exercises a narrow internal code path.

use crate::test_support;
use radix_target::Target;

use radix_hir::{DefId, HirItemKind, HirModule, HirStatementKind};

use super::typecheck::{TypeChecker, typecheck};
use crate::SemanticErrorKind;
use crate::file_interface::{FileExport, FileExportKind, FileInterface, InterfaceStructExport};
use crate::passes::visibility_annotation::VisibilityTier;
use crate::test_support::args_have_no_legacy_message;
use crate::test_support::test_span;
use crate::{Primitive, Resolver, Type, TypeTable};
use radix_hir::{HirCallArg, HirExpression, HirExpressionKind, HirLiteral, HirPattern};
use radix_lexer::Span;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helper: compile source and get diagnostics (fallback for non-harness assertions)
// ---------------------------------------------------------------------------

fn compile_and_diagnostics(source: &str) -> Vec<crate::SemanticError> {
    test_support::compile_and_get_diagnostics(source)
}

// ---------------------------------------------------------------------------
// Helper: analyze source and return the analyzed unit
// ---------------------------------------------------------------------------

fn analyze(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source(source)
}

// ===========================================================================
// Source-based tests — accept
// ===========================================================================

#[test]
fn omitted_return_effect_function_finalizes_to_vacuum() {
    // HIR version: effect-only function (⇥ textus, no → ret) whose body
    // throws — the return type should be inferred as vacuum.
    let unit = analyze("functio f() ⇥ textus { iace \"bad\" }");
    let item = &unit.hir.as_ref().expect("hir").items[0];
    let HirItemKind::Function(func) = &item.kind else {
        panic!("expected function item");
    };
    assert_eq!(func.ret_ty, Some(unit.types.primitive(Primitive::Vacuum)));
}

#[test]
fn boolean_operator_constrains_inferred_operand() {
    // HIR reached an unreachable state: local with no type annotation and no
    // initializer, then used as && operand so the bool operator constrains the
    // inferred type to bivalens. Valid Faber requires either a type or an
    // initializer for fixum, so this test is kept in HIR form.
    let mut types = TypeTable::new();
    let local_def = DefId(0);
    let mut program = HirModule {
        items: Vec::new(),
        entry: Some(radix_hir::HirBlock {
            statements: vec![radix_hir::HirStatement {
                id: radix_hir::HirId(0),
                kind: radix_hir::HirStatementKind::Local(radix_hir::HirLocal {
                    def_id: local_def,
                    name: radix_lexer::Symbol(1),
                    ty: None,
                    init: None,
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: test_span(),
            }],
            expr: Some(Box::new(radix_hir::HirExpression {
                id: radix_hir::HirId(1),
                kind: HirExpressionKind::Binary(
                    radix_hir::HirBinOp::And,
                    Box::new(radix_hir::HirExpression {
                        id: radix_hir::HirId(2),
                        kind: HirExpressionKind::Path(local_def),
                        ty: None,
                        span: test_span(),
                    }),
                    Box::new(radix_hir::HirExpression {
                        id: radix_hir::HirId(3),
                        kind: HirExpressionKind::Literal(HirLiteral::Bool(true)),
                        ty: None,
                        span: test_span(),
                    }),
                ),
                ty: None,
                span: test_span(),
            })),
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = typecheck(&mut program, &Resolver::new(), &mut types);
    assert!(result.is_ok());
}

#[test]
fn bit_operator_constrains_inferred_operand() {
    // Same unreachable-HIR pattern as boolean_operator_constrains_inferred_operand
    // but with BitAnd. Kept as HIR construction — cannot write valid Faber
    // source that produces a typeless, initless local.
    let mut types = TypeTable::new();
    let local_def = DefId(0);
    let mut program = HirModule {
        items: Vec::new(),
        entry: Some(radix_hir::HirBlock {
            statements: vec![radix_hir::HirStatement {
                id: radix_hir::HirId(0),
                kind: radix_hir::HirStatementKind::Local(radix_hir::HirLocal {
                    def_id: local_def,
                    name: radix_lexer::Symbol(1),
                    ty: None,
                    init: None,
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: test_span(),
            }],
            expr: Some(Box::new(radix_hir::HirExpression {
                id: radix_hir::HirId(1),
                kind: HirExpressionKind::Binary(
                    radix_hir::HirBinOp::BitAnd,
                    Box::new(radix_hir::HirExpression {
                        id: radix_hir::HirId(2),
                        kind: HirExpressionKind::Path(local_def),
                        ty: None,
                        span: test_span(),
                    }),
                    Box::new(radix_hir::HirExpression {
                        id: radix_hir::HirId(3),
                        kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
                        ty: None,
                        span: test_span(),
                    }),
                ),
                ty: None,
                span: test_span(),
            })),
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let result = typecheck(&mut program, &Resolver::new(), &mut types);
    assert!(result.is_ok());
}

#[test]
fn struct_field_default_provides_context_for_vacua() {
    // Struct field with default = vacua in a collection-typed field
    test_support::assert_accepts("genus S { varia lista<numerus> f = vacua }");
}

#[test]
fn same_named_fn_does_not_shadow_bare_vacua_marker() {
    // em1: a function named like the marker does not shadow the bare marker.
    // The collection-typed field and initializer use the marker, while
    // `vacua()` still calls the function.
    test_support::assert_accepts(
        r#"
genus Header { varia textus name = "" }
genus Headers { varia lista<Header> entries = vacua }
functio vacua() → Headers { redde Headers { } }
functio probe() → lista<Header> {
  varia lista<Header> out ← vacua
  redde out
}
functio use_call() → Headers { redde vacua() }
"#,
    );
}

#[test]
fn same_named_en_empty_fn_does_not_shadow_bare_marker() {
    // em1: the English reader pack claims `empty` as the marker; a live
    // `fn empty() → Headers` does not shadow it, so collection positions use
    // the marker and `empty()` still calls the function.
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path)
        .expect("installed en pack should load");
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(pack);
    let result = crate::test_support::analyze_source_with_config(
        r#"
class Header {
    mut string name = ""
}
class Headers {
    mut list<Header> entries = empty
}
fn empty() → Headers {
    return Headers { }
}
fn probe() → list<Header> {
    mut list<Header> out ← empty
    return out
}
fn use_call() → Headers {
    return empty()
}
"#,
        config,
    );
    assert!(
        result.is_ok(),
        "a function named like the marker must not shadow it: {:?}",
        result.diagnostics
    );
}

#[test]
fn struct_literal_field_provides_context_for_vacua() {
    // Struct construction with vacua value for a collection-typed field.
    // The parser produces a Verte for `S { ... }`, which typecheck resolves
    // to the struct target.
    test_support::assert_accepts(
        "genus S { varia lista<numerus> f } incipit { sit _ s ← S { f = vacua } }",
    );
}

#[test]
fn verte_struct_field_provides_context_for_vacua() {
    // Verte operator: `S { fields }` is the canonical typed-constructor syntax.
    // The lowerer detects the Object inside Verte and uses Tuple as placeholder source.
    test_support::assert_accepts(
        "genus S { varia lista<numerus> f } incipit { sit _ s ← S { f = vacua } }",
    );
}

#[test]
fn typechecks_iace_against_declared_alternate_exit() {
    // Faber uses newlines, not semicolons, between statements in a block
    test_support::assert_accepts("functio f() → numerus ⇥ textus {\n  iace \"bad\"\n  redde 1\n}");
}

#[test]
fn variant_constructor_field_provides_context_for_vacua() {
    // Tagged union variant constructor with vacua in a collection-typed field
    // Use lista<numerus> not numerus[] (postfix [] only valid in function params)
    test_support::assert_accepts(
        "discretio E { V { lista<numerus> f } } incipit { sit _ e ← V(vacua) }",
    );
}

#[test]
fn cross_union_same_name_variants_typecheck_distinct_values() {
    // uvn-u2 done_when (a): `A.X` and `B.X` typecheck to distinct variant
    // values; qualified construction (unit-variant value form) and qualified
    // member access compile. Payload-bearing qualified construction has no
    // surface in the language yet — bare finge with a cast stays the
    // single-union construction form (covered by the resolve-level tests).
    test_support::assert_accepts(
        r#"discretio A { X }
discretio B { X }

functio read_a(A a) → textus {
    discerne a {
        casu A.X { redde "A.X" }
    }
}

functio read_b(B b) → textus {
    discerne b {
        casu B.X { redde "B.X" }
    }
}

incipit {
    sit A a ← A.X
    sit B b ← B.X
    sit textus sa ← read_a(a)
    sit textus sb ← read_b(b)
    nota sa + sb
}"#,
    );
}

#[test]
fn same_union_duplicate_variant_rejected_by_full_pipeline() {
    // uvn-u2 done_when (d): same-union duplicate variant still SEM005.
    test_support::assert_rejects_with_issue("discretio A { X, X }", "duplicate_definition");
}

#[test]
fn resolves_method_call_type() {
    // Method call inside a redde expression — typecheck infers the method's
    // return type onto the redde expression.
    test_support::assert_accepts(
        "genus S {\n  functio m() → numerus { redde 42 }\n}\
         functio test() → numerus { sit S s ← S {} redde s.m() }",
    );
}

#[test]
fn known_method_on_concrete_receiver_still_resolves() {
    // lookup_method_signature keep: declared methods on a genus receiver still typecheck.
    test_support::assert_accepts(
        "genus Seed {\n  functio status() → numerus { redde 1 }\n}\
         functio probe() → numerus { sit Seed s ← Seed {} redde s.status() }",
    );
}

#[test]
fn callable_struct_field_is_invocable_as_member_call() {
    // A function-valued field must be callable through the member-call syntax
    // that lowering uses for every `receiver.member(args)` expression.
    test_support::assert_accepts(
        r#"
discretio Nuntius { Electus { textus identitas } }
genus Eventum { varia textus nomen }
genus Ligatio { varia (Eventum) → Nuntius nuntius }
functio selectum(textus identitas) → Nuntius {
    redde finge Electus { identitas = identitas } ∷ Nuntius
}
functio fac() → Ligatio {
    redde Ligatio {
        nuntius = (Eventum eventum) → Nuntius ∴ selectum(eventum.nomen)
    }
}
functio usus() → Nuntius {
    sit Ligatio ligatio ← fac()
    redde ligatio.nuntius(Eventum { nomen = "x" })
}
"#,
    );
}

#[test]
fn non_callable_struct_field_member_call_remains_unknown_method() {
    test_support::assert_rejects_with_issue(
        r#"
genus Ligatio { varia textus nuntius }
functio usus(Ligatio ligatio) → textus {
    redde ligatio.nuntius("x")
}
"#,
        "unknown_method",
    );
}

#[test]
fn callable_struct_field_member_call_checks_argument_count() {
    test_support::assert_rejects_with_issue(
        r"
genus Eventum { varia textus nomen }
genus Ligatio { varia (Eventum) → textus nuntius }
functio usus(Ligatio ligatio) → textus {
    redde ligatio.nuntius()
}
",
        "wrong_number_of_arguments",
    );
}

#[test]
fn callable_struct_field_member_call_checks_argument_types() {
    test_support::assert_rejects_with_issue(
        r#"
genus Eventum { varia textus nomen }
genus Ligatio { varia (Eventum) → textus nuntius }
functio usus(Ligatio ligatio) → textus {
    redde ligatio.nuntius("x")
}
"#,
        "argument_type_mismatch",
    );
}

#[test]
fn explicit_method_wins_over_same_named_callable_struct_field() {
    test_support::assert_accepts(
        r#"
genus Eventum { varia textus nomen }
genus Ligatio {
    varia (Eventum) → textus nuntius
    functio nuntius() → textus { redde "method" }
}
functio usus(Ligatio ligatio) → textus {
    redde ligatio.nuntius()
}
"#,
    );
}

#[test]
fn ignotum_receiver_method_call_still_infers() {
    // Dynamic/Ignotum receivers keep inferring unknown methods.
    test_support::assert_accepts("functio probe(ignotum s) → ignotum { redde s.from_flat(3) }");
}

#[test]
fn accepts_failable_call_in_ordinary_expression_position() {
    // Failable call in a redde within a ⇥-declaring function should propagate
    test_support::assert_accepts(
        "functio fail() → numerus ⇥ textus { iace \"bad\" } \
         functio caller() → numerus ⇥ textus { redde fail() }",
    );
}

#[test]
fn accepts_cede_yield_in_cursor_generator() {
    test_support::assert_accepts(
        "@ cursor
        functio stream() → numerus {
            cede 1
        }",
    );
}

#[test]
fn accepts_figendum_await_bind_in_async_function() {
    test_support::assert_accepts(
        r#"functio fetch() fiet → textus {
  redde "data"
}

functio caller() fiet → textus {
  figendum _ value ← fetch()
  redde value
}"#,
    );
}

#[test]
fn accepts_await_directed_binding_of_a_stored_promise() {
    let unit = analyze(
        r#"functio fetch() fiet → textus {
  redde "data"
}

functio caller() fiet → textus {
  sit promissum<textus> pending ← fetch()
  sit textus value ↢ pending
  redde value
}"#,
    );
    assert!(unit.success(), "diagnostics: {:?}", unit.diagnostics);

    let HirItemKind::Function(function) = &unit.hir.as_ref().expect("hir").items[1].kind else {
        panic!("expected caller function");
    };
    let HirStatementKind::Local(local) =
        &function.body.as_ref().expect("caller body").statements[1].kind
    else {
        panic!("expected awaited local");
    };
    assert!(local.is_await);
    assert!(
        unit.presentation
            .await_assignment_bindings
            .contains(&local.def_id)
    );
}

#[test]
fn accepts_await_directed_failable_promise_with_matching_alternate_exit() {
    test_support::assert_accepts(
        r#"functio fetch() fiet → numerus ⇥ textus {
  iace "bad"
}

functio caller() fiet → numerus ⇥ textus {
  sit _ pending ← fetch()
  sit _ value ↢ pending
  redde value
}"#,
    );
}

#[test]
fn accepts_stored_failable_async_future_without_immediate_alternate_exit() {
    test_support::assert_accepts(
        r#"functio fetch() fiet → numerus ⇥ textus {
  iace "bad"
}

functio store() → promissum<numerus ⇥ textus> {
  redde fetch()
}"#,
    );
}

#[test]
fn accepts_figendum_failable_future_in_matching_failable_async_function() {
    test_support::assert_accepts(
        r#"functio fetch() fiet → numerus ⇥ textus {
  iace "bad"
}

functio caller() fiet → numerus ⇥ textus {
  figendum _ value ← fetch()
  redde value
}"#,
    );
}

#[test]
fn accepts_reddet_failable_future_in_matching_failable_async_function() {
    test_support::assert_accepts(
        r#"functio fetch() fiet → numerus ⇥ textus {
  iace "bad"
}

functio caller() fiet → numerus ⇥ textus {
  reddet fetch()
}"#,
    );
}

#[test]
fn accepts_tacebit_failable_future_in_matching_failable_async_function() {
    test_support::assert_accepts(
        r#"functio effect() fiet → vacuum ⇥ textus {
  iace "bad"
}

functio caller() fiet → vacuum ⇥ textus {
  tacebit effect()
}"#,
    );
}

#[test]
fn accepts_tacebit_await_discard_in_async_entry() {
    test_support::assert_accepts(
        r"functio sleep() fiet → vacuum {
}

incipiet {
  tacebit sleep()
}",
    );
}

#[test]
fn accepts_reddet_in_async_finite_function() {
    test_support::assert_accepts(
        r#"functio fetch() fiet → textus {
  redde "data"
}

functio caller() fiet → textus {
  reddet fetch()
}"#,
    );
}

#[test]
fn implendum_async_method_call_returns_promissum() {
    test_support::assert_accepts(
        r"implendum Source {
  functio read() fiet → numerus
}

functio caller(Source source) → promissum<numerus> {
  redde source.read()
}",
    );
}

// ===========================================================================
// Source-based tests — reject with specific issue or kind
// ===========================================================================

#[test]
fn rejects_redde_without_explicit_return_type() {
    test_support::assert_rejects_with_issue(
        "functio f() { redde 42 }",
        "redde_requires_return_type",
    );
}

#[test]
fn rejects_cede_outside_generator() {
    test_support::assert_rejects_with_issue(
        "incipit { cede \"not a promise\" }",
        "cede_requires_generator",
    );
}

#[test]
fn rejects_figendum_outside_async_context() {
    test_support::assert_rejects_with_issue(
        r#"functio fetch() fiet → textus {
  redde "data"
}

incipit {
  figendum _ value ← fetch()
}"#,
        "await_bind_requires_async_context",
    );
}

#[test]
fn rejects_await_directed_binding_outside_async_context() {
    test_support::assert_rejects_with_issue(
        r#"functio fetch() fiet → textus {
  redde "data"
}

incipit {
  sit textus value ↢ fetch()
}"#,
        "await_bind_requires_async_context",
    );
}

#[test]
fn rejects_await_directed_binding_of_a_non_promise() {
    test_support::assert_rejects_with_issue(
        "incipiet { sit numerus value ↢ 1 }",
        "await_bind_requires_promissum",
    );
}

#[test]
fn rejects_tacebit_outside_async_context() {
    test_support::assert_rejects_with_issue(
        r"functio sleep() fiet → vacuum {
}

incipit {
  tacebit sleep()
}",
        "tacebit_requires_async_context",
    );
}

#[test]
fn rejects_tacebit_non_promissum_operand() {
    test_support::assert_rejects_with_issue(
        r"incipiet {
  tacebit 1
}",
        "cede_requires_promissum",
    );
}

#[test]
fn rejects_storing_failable_async_future_as_infallible_promissum() {
    test_support::assert_rejects_with_issue(
        r#"functio fetch() fiet → numerus ⇥ textus {
  iace "bad"
}

functio store() → promissum<numerus> {
  redde fetch()
}"#,
        "return_type_mismatch",
    );
}

#[test]
fn rejects_figendum_failable_future_without_alternate_exit() {
    test_support::assert_rejects_with_issue(
        r#"functio fetch() fiet → numerus ⇥ textus {
  iace "bad"
}

functio caller() fiet → numerus {
  figendum _ value ← fetch()
  redde value
}"#,
        "await_failable_requires_alternate_exit",
    );
}

#[test]
fn rejects_tacebit_failable_future_with_mismatched_alternate_exit() {
    test_support::assert_rejects_with_issue(
        r#"functio effect() fiet → vacuum ⇥ textus {
  iace "bad"
}

functio caller() fiet → vacuum ⇥ numerus {
  tacebit effect()
}"#,
        "await_failable_alternate_type_mismatch",
    );
}

#[test]
fn rejects_reddet_outside_async_finite_function() {
    test_support::assert_rejects_with_issue(
        r#"functio fetch() fiet → textus {
  redde "data"
}

incipiet {
  reddet fetch()
}"#,
        "reddet_requires_async_function",
    );
}

#[test]
fn rejects_reddet_in_async_stream_function() {
    test_support::assert_rejects_with_issue_for_target(
        r#"functio fetch() fiet → textus {
  redde "data"
}

functio stream() fient → textus {
  reddet fetch()
}"#,
        Target::HirFaber,
        "reddet_requires_async_function",
    );
}

#[test]
fn fient_call_has_async_cursor_type_boundary() {
    let unit = crate::test_support::analyze_source(
        r"functio stream() fient → numerus ⇥ textus {
  cede 1
}

incipit {
  sit _ handle ← stream()
}",
    );
    let entry = unit
        .hir
        .as_ref()
        .expect("hir")
        .entry
        .as_ref()
        .expect("entry");
    let HirStatementKind::Local(local) = &entry.statements[0].kind else {
        panic!("expected local statement");
    };
    let init_ty = local
        .init
        .as_ref()
        .and_then(|init| init.ty)
        .expect("typed stream call");
    let Type::AsyncCursor(item, alternate) = unit.types.get(init_ty) else {
        panic!(
            "expected fient call to produce async cursor, got {:?}",
            unit.types.get(init_ty)
        );
    };
    assert_eq!(*item, unit.types.primitive(Primitive::Numerus));
    assert_eq!(*alternate, unit.types.primitive(Primitive::Textus));
}

#[test]
fn reports_initializer_type_mismatch() {
    let diags = compile_and_diagnostics("incipit { sit textus x ← 7 }");
    assert!(diags.iter().any(crate::SemanticError::is_error));
}

#[test]
fn condition_type_mismatch_does_not_emit_follow_on_condition_error() {
    // si with a numerus condition → TypeMismatch, but NOT condition_must_be_bivalens
    let diags = compile_and_diagnostics("incipit { si 1 { } }");
    assert!(diags.iter().any(crate::SemanticError::is_error));
    assert!(
        !diags
            .iter()
            .any(|d| d.issue() == Some("condition_must_be_bivalens"))
    );
}

#[test]
fn vacua_with_non_collection_context_uses_structured_issue_arg() {
    test_support::assert_rejects_with_issue(
        "sit numerus name ← vacua",
        "vacua_requires_collection_type",
    );
}

#[test]
fn object_field_without_value_uses_structured_issue_arg() {
    // This HIR state (Ident key with value=None) is unreachable from valid
    // Faber source — the parser auto-fills shorthand idents and requires values
    // for string/computed keys. Kept as HIR construction.
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let map_ty = types.intern(Type::Map(textus, numerus));
    let expr_span = Span::new(20, 35);
    let field_name = radix_lexer::Symbol(2);
    let mut program = HirModule {
        items: Vec::new(),
        entry: Some(radix_hir::HirBlock {
            statements: Vec::new(),
            expr: Some(Box::new(HirExpression {
                id: radix_hir::HirId(10),
                kind: HirExpressionKind::Verte {
                    source: Box::new(HirExpression {
                        id: radix_hir::HirId(11),
                        kind: HirExpressionKind::Tuple(Vec::new(), None),
                        ty: None,
                        span: test_span(),
                    }),
                    target: map_ty,
                    entries: Some(vec![radix_hir::HirObjectField {
                        key: radix_hir::HirObjectKey::Ident(field_name),
                        value: None,
                    }]),
                },
                ty: None,
                span: expr_span,
            })),
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = typecheck(&mut program, &Resolver::new(), &mut types).unwrap_err();
    assert!(errors.iter().any(|err| {
        err.kind == SemanticErrorKind::OperandTypesInvalid
            && err.span == expr_span
            && args_have_no_legacy_message(&err.args)
            && err
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "object_field_requires_value")
    }));
}

#[test]
fn array_spread_non_lista_uses_structured_issue_arg() {
    // Spread in an array context that doesn't resolve to lista
    // A bare array with spread → spread requires lista context
    test_support::assert_rejects_with_issue(
        "incipit { sit _ a ← [sparge 1] }",
        "array_spread_requires_lista",
    );
}

#[test]
fn struct_computed_key_uses_structured_issue_arg() {
    // Computed key in struct verte with value=None — the parser requires a
    // value expression after computed keys, so this HIR state is unreachable
    // from valid source. Kept as HIR construction.
    let mut types = TypeTable::new();
    let key_span = Span::new(20, 25);
    let numerus = types.primitive(Primitive::Numerus);
    let struct_def = DefId(0);
    let field_name = radix_lexer::Symbol(2);
    let struct_ty = types.intern(Type::Struct(struct_def));
    let key_expr = HirExpression {
        id: radix_hir::HirId(11),
        kind: HirExpressionKind::Literal(HirLiteral::String(radix_lexer::Symbol(99))),
        ty: None,
        span: key_span,
    };
    let mut program = HirModule {
        items: vec![radix_hir::HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: struct_def,
            kind: HirItemKind::Struct(radix_hir::HirStruct {
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                fields: vec![radix_hir::HirField {
                    def_id: DefId(1),
                    name: field_name,
                    ty: numerus,
                    modifier: None,
                    sponte: false,
                    annotations: Vec::new(),
                    init: None,
                    span: test_span(),
                }],
                methods: Vec::new(),
                implements: Vec::new(),
            }),
            span: test_span(),
        }],
        entry: Some(radix_hir::HirBlock {
            statements: Vec::new(),
            expr: Some(Box::new(HirExpression {
                id: radix_hir::HirId(10),
                kind: HirExpressionKind::Verte {
                    source: Box::new(HirExpression {
                        id: radix_hir::HirId(11),
                        kind: HirExpressionKind::Tuple(Vec::new(), None),
                        ty: None,
                        span: test_span(),
                    }),
                    target: struct_ty,
                    entries: Some(vec![radix_hir::HirObjectField {
                        key: radix_hir::HirObjectKey::Computed(key_expr),
                        value: None,
                    }]),
                },
                ty: None,
                span: key_span,
            })),
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = typecheck(&mut program, &Resolver::new(), &mut types).unwrap_err();
    assert!(errors.iter().any(|err| {
        err.kind == SemanticErrorKind::OperandTypesInvalid
            && err.span == key_span
            && test_support::semantic_error_has_issue(err, "struct_computed_key_forbidden")
    }));
}

#[test]
fn assignment_to_constant_uses_structured_issue_arg() {
    // Top-level sit lowered to HirConst, then assignment via ← in incipit.
    test_support::assert_rejects_with_issue(
        "sit numerus NAME ← 1\nincipit { NAME ← 2 }",
        "assignment_to_constant",
    );
}

#[test]
fn item_expected_type_mismatches_use_structured_issue_args() {
    // Top-level constant mismatch, param default mismatch, field default mismatch.
    // Use ← for top-level fixum; vel for function param defaults; = for struct field defaults.
    let diags = compile_and_diagnostics(
        "sit numerus C ← \"bad\"\n\
         functio f(numerus p vel \"bad\") → textus { redde \"ok\" }\n\
         genus S { varia numerus f = \"bad\" }",
    );
    for issue in &[
        "constant_annotation_mismatch",
        "parameter_default_type_mismatch",
        "field_default_type_mismatch",
        "expression_type_mismatch",
    ] {
        assert!(
            diags
                .iter()
                .any(|d| d.is_error() && d.issue() == Some(issue)),
            "expected issue {issue}, got: {diags:?}"
        );
    }
}

#[test]
fn assert_condition_type_mismatch_uses_structured_issue_arg() {
    test_support::assert_rejects_with_issue(
        "incipit { adfirma 1 }",
        "assert_condition_type_mismatch",
    );
}

#[test]
fn struct_literal_field_initializer_type_mismatch_uses_structured_issue_arg() {
    test_support::assert_rejects_with_issue(
        "genus S { varia numerus f } incipit { sit _ s ← S { f = \"bad\" } }",
        "field_initializer_type_mismatch",
    );
}

#[test]
fn verte_field_initializer_type_mismatch_uses_structured_issue_arg() {
    // Verte struct constructor with wrong field type
    test_support::assert_rejects_with_issue(
        "genus S { varia numerus f } incipit { sit _ s ← S { f = \"bad\" } }",
        "field_initializer_type_mismatch",
    );
}

#[test]
fn closure_parameter_type_mismatch_uses_structured_issue_arg() {
    // Fixum with explicit function type and a closure that has wrong param type.
    // Type syntax: sit Type name ← expr
    test_support::assert_rejects_with_issue(
        "incipit { sit (numerus) → numerus f ← clausura textus x → numerus { redde 1 } }",
        "closure_parameter_type_mismatch",
    );
}

#[test]
fn closure_return_type_mismatch_uses_structured_issue_arg() {
    // Compact closure form avoids redde (which would produce return_type_mismatch
    // before the closure-level check runs). The expression body is the return value.
    test_support::assert_rejects_with_issue(
        "incipit { sit () → numerus f ← () → numerus ∴ \"bad\" }",
        "closure_return_type_mismatch",
    );
}

#[test]
fn unresolved_local_type_uses_declaration_span() {
    // local without type annotation or initializer → cannot infer type
    // However, Faber requires at least one of type or initializer for fixum.
    // This test HIR state is not reachable from valid source, so we keep it
    // as HIR construction.
    let mut types = TypeTable::new();
    let decl_span = Span::new(10, 20);
    let mut program = HirModule {
        items: Vec::new(),
        entry: Some(radix_hir::HirBlock {
            statements: vec![radix_hir::HirStatement {
                id: radix_hir::HirId(0),
                kind: radix_hir::HirStatementKind::Local(radix_hir::HirLocal {
                    def_id: DefId(0),
                    name: radix_lexer::Symbol(1),
                    ty: None,
                    init: None,
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: decl_span,
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = typecheck(&mut program, &Resolver::new(), &mut types).unwrap_err();
    assert!(errors.iter().any(|err| {
        err.kind == SemanticErrorKind::TypeAnnotationAbsent
            && err.span == decl_span
            && args_have_no_legacy_message(&err.args)
            && err
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "cannot_infer_variable_type")
    }));
}

#[test]
fn rejects_iace_without_declared_alternate_exit() {
    test_support::assert_rejects_with_issue(
        "functio f() → numerus { iace \"bad\" }",
        "iace_requires_alternate_exit",
    );
}

#[test]
fn rejects_requirit_without_declared_alternate_exit() {
    // The desugared `iace` payload inherits iace's error-channel rule: a
    // `requirit` statement in a ⇥-less function is a compile error.
    test_support::assert_rejects_with_issue(
        "functio f() → numerus {\n  requirit verum iace \"bad\"\n  redde 1\n}",
        "iace_requires_alternate_exit",
    );
}

#[test]
fn typechecks_requirit_against_declared_alternate_exit() {
    test_support::assert_accepts(
        "functio f() → numerus ⇥ textus {\n  requirit verum iace \"bad\"\n  redde 1\n}",
    );
}

#[test]
fn rejects_iace_value_that_mismatches_alternate_exit() {
    test_support::assert_rejects_with_issue(
        "functio f() → numerus ⇥ numerus { iace \"bad\" }",
        "iace_value_type_mismatch",
    );
}

#[test]
fn redde_still_checks_against_normal_return_type() {
    test_support::assert_rejects_with_issue(
        "functio f() → numerus ⇥ textus { redde \"bad\" }",
        "return_type_mismatch",
    );
}

#[test]
fn rejects_function_value_with_mismatched_parameter_mode() {
    // A function with `de` (borrow) param where `in` (owned) is expected.
    // Assign `de`-param function to a local annotated with `in`-param function type.
    test_support::assert_rejects_with_issue(
        "functio f(de numerus x) → numerus { redde x } \
         incipit { sit (numerus) → numerus g ← f }",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn inferred_callee_nested_argument_emits_one_argument_mismatch_diagnostic() {
    // This test verifies that when a nested call argument is passed to an
    // inferred outer callee (one whose type is not yet known), the typechecker
    // does NOT re-typecheck the inner call's arguments when inference resolves
    // the outer callee. The HIR state cannot be expressed in valid Faber source
    // because it requires a typeless, initless local used as a callee with
    // nested malformed calls. Kept as HIR construction.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let bad_def = DefId(1);
    let outer_def = DefId(10);
    let bad_call = HirExpression {
        id: radix_hir::HirId(20),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: radix_hir::HirId(21),
                kind: HirExpressionKind::Path(bad_def),
                ty: None,
                span: test_span(),
            }),
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: HirExpression {
                    id: radix_hir::HirId(22),
                    kind: HirExpressionKind::Literal(HirLiteral::String(radix_lexer::Symbol(99))),
                    ty: None,
                    span: test_span(),
                },
                span: test_span(),
            }],
            None,
        ),
        ty: None,
        span: test_span(),
    };
    let mut program = HirModule {
        items: vec![radix_hir::HirItem {
            annotations: Vec::new(),
            id: radix_hir::HirId(0),
            def_id: bad_def,
            kind: HirItemKind::Function(Box::new(radix_hir::HirFunction {
                cli_args: None,
                name: radix_lexer::Symbol(1),
                type_params: Vec::new(),
                params: vec![radix_hir::HirParam {
                    def_id: DefId(2),
                    name: radix_lexer::Symbol(3),
                    ty: numerus,
                    mode: radix_hir::HirParamMode::Owned,
                    optional: false,
                    sponte: false,
                    default: None,
                    span: test_span(),
                }],
                ret_ty: Some(numerus),
                err_ty: None,
                body: None,
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,
                test: None,
            })),
            span: test_span(),
        }],
        entry: Some(radix_hir::HirBlock {
            statements: vec![radix_hir::HirStatement {
                id: radix_hir::HirId(30),
                kind: radix_hir::HirStatementKind::Local(radix_hir::HirLocal {
                    def_id: outer_def,
                    name: radix_lexer::Symbol(4),
                    ty: None,
                    init: None,
                    mutable: false,
                    is_await: false,
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                }),
                span: test_span(),
            }],
            expr: Some(Box::new(HirExpression {
                id: radix_hir::HirId(31),
                kind: HirExpressionKind::Call(
                    Box::new(HirExpression {
                        id: radix_hir::HirId(32),
                        kind: HirExpressionKind::Path(outer_def),
                        ty: None,
                        span: test_span(),
                    }),
                    Vec::new(),
                    vec![HirCallArg {
                        name: None,
                        spread: false,
                        marker: None,
                        expr: bad_call,
                        span: test_span(),
                    }],
                    None,
                ),
                ty: None,
                span: test_span(),
            })),
            span: test_span(),
            ..Default::default()
        }),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    let errors = typecheck(&mut program, &Resolver::new(), &mut types).unwrap_err();
    let mismatches: Vec<_> = errors
        .iter()
        .filter(|err| {
            err.kind == SemanticErrorKind::TypeMismatch
                && test_support::semantic_error_has_issue(err, "argument_type_mismatch")
        })
        .collect();
    assert_eq!(
        mismatches.len(),
        1,
        "inferred callee must not re-typecheck nested call arguments: {errors:?}"
    );
}

#[test]
fn wrong_call_arity_uses_structured_issue_arg() {
    test_support::assert_rejects_with_issue(
        "functio f(numerus x) → numerus { redde x } incipit { nota f() }",
        "wrong_number_of_arguments",
    );
}

#[test]
fn non_callable_callee_uses_structured_issue_arg() {
    test_support::assert_rejects_with_issue("incipit { nota 1() }", "callee_not_callable");
}

#[test]
fn invalid_conversion_uses_structured_issue_arg() {
    test_support::assert_rejects_with_issue(
        "incipit { nota 1 ↦ lista<b8> }",
        "numeric_to_octeti_unspecified",
    );
}

#[test]
fn json_map_key_type_mismatch_uses_structured_issue_arg() {
    // JSON object where a key should be textus but we use a non-textus key
    // However, JSON literal keys are always strings in the parser, so the HIR
    // scenario (a numeric key in a JSON object) may not be reachable.
    // Use a tabula<numerus, numerus> type with a JSON object — the JSON key
    // must be textus.
    test_support::assert_rejects_with_issue(
        "incipit { sit tabula<numerus, numerus> m ← { \"key\": 1 } }",
        "json_map_key_must_be_textus",
    );
}

#[test]
fn nested_json_value_type_mismatch_uses_structured_issue_arg() {
    // Nested JSON value that's not a valor — a nested object inside a non-valor typed map
    test_support::assert_rejects_with_issue(
        "incipit { sit tabula<textus, numerus> m ← { \"key\": { \"inner\": 1 } } }",
        "json_nested_value_requires_valor",
    );
}

#[test]
fn json_value_type_mismatch_uses_structured_issue_arg() {
    // JSON string value where numerus is expected
    test_support::assert_rejects_with_issue(
        "incipit { sit tabula<textus, numerus> m ← { \"key\": \"not_a_number\" } }",
        "json_value_type_mismatch",
    );
}

#[test]
fn dynamic_collection_invalid_conversion_uses_structured_context_args() {
    test_support::assert_rejects_with_issue(
        "incipit { nota [1] ↦ tabula<textus, numerus> }",
        "lista_to_tabula_unsupported",
    );
}

#[test]
fn intervallum_invalid_operand_uses_structured_issue_arg() {
    // The Faber range membership operator is `∈`, and the range operator is `‥`
    test_support::assert_rejects_with_issue(
        "incipit { nota \"text\" ∈ 0‥3 }",
        "range_operand_must_be_numeric",
    );
}

#[test]
fn string_add_invalid_operand_uses_structured_issue_arg() {
    test_support::assert_rejects_with_issue(
        "incipit { nota \"text\" + 1 }",
        "string_operands_required",
    );
}

#[test]
fn reports_unknown_struct_field_access() {
    test_support::assert_rejects_with_issue(
        "genus S { varia numerus f } incipit { sit S s nota s.g }",
        "unknown_struct_field",
    );
}

#[test]
fn rejects_unknown_method_on_concrete_receiver() {
    // Concrete genus receiver with no such method must emit unknown_method.
    test_support::assert_rejects_with_issue(
        "genus Seed {\n  functio status() → numerus { redde 1 }\n}\
         functio probe() → numerus { sit Seed s ← Seed {} redde s.from_flat(3) }",
        "unknown_method",
    );
}

#[test]
fn rejects_unknown_tensor_method_at_typecheck() {
    // Tensor intrinsic misses must fail at check rather than infer a result
    // that only fails later when a device target tries to lower it. `silu`
    // joined the registered tensor surface (GEA2), so the probe rides a
    // spelling the registry will never own.
    test_support::assert_rejects_with_issue(
        "functio probe(tf32[2, 2] x) → tf32[2, 2] { redde x.nusquam_methodus() }",
        "unknown_method",
    );
}

#[test]
fn rejects_unknown_vector_method_at_typecheck() {
    test_support::assert_rejects_with_issue(
        "functio probe(vector<f32, magnitudo 2> x) → vector<f32, magnitudo 2> { redde x.silu() }",
        "unknown_method",
    );
}

#[test]
fn rejects_unknown_matrix_method_at_typecheck() {
    test_support::assert_rejects_with_issue(
        "functio probe(matrix<f32, [2, 2]> x) → matrix<f32, [2, 2]> { redde x.silu() }",
        "unknown_method",
    );
}

#[test]
fn rejects_unknown_method_on_imported_struct_receiver() {
    // DX receipt a35e77d5: the tensor receiver was accidentally replaced by
    // `train.Seed` in four `from_flat` calls. Keep the imported nominal shape
    // here so typecheck cannot recover through an inferred method result.
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Seed".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: "Seed".to_owned(),
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    let result = test_support::analyze_source_with_import_contract(
        r#"importa ex "./train" train

functio probe(train.Seed seed) → tensor<f32, [2]> {
    redde seed.from_flat([1.0, 2.0], [2])
}
"#,
        BTreeMap::from([(
            "train".to_owned(),
            BTreeMap::from([("Seed".to_owned(), VisibilityTier::Publica)]),
        )]),
        BTreeMap::from([("train".to_owned(), interface)]),
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MemberUndefined
                && diagnostic
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "unknown_method")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected unknown_method for imported Seed receiver, got {:?}",
                result.diagnostics
            )
        });
    assert!(
        args_have_no_legacy_message(&diagnostic.args),
        "unknown_method must remain a structured diagnostic: {diagnostic:?}"
    );
}

#[test]
fn reports_unknown_identifier_with_structured_event() {
    test_support::assert_rejects_with_issue("incipit { nota nonexistent }", "unknown_identifier");
}

// ===========================================================================
// Tests that remain as HIR construction
// ===========================================================================
// Three categories are preserved as HIR:
//
// 1. Internal checker methods (check_call, check_pattern) – these exercise
//    narrow code paths that cannot be reached through the normal analysis
//    pipeline.
// 2. Unreachable-from-source HIR states – typeless/initless locals, edge-case
//    inference paths that the parser rejects.
// 3. Tests that directly inspect the program state after typecheck for
//    properties the harness API doesn't expose.

// ---------------------------------------------------------------------------
// Internal-checker tests
// ---------------------------------------------------------------------------

/// Creates a minimal HIR expression for use in internal-checker tests.
fn literal_int(id: u32, value: u64) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: None,
        span: test_span(),
    }
}

#[test]
fn variant_call_reports_missing_field_metadata() {
    // Directly calls checker.check_call() to test a narrow error path where
    // variant field metadata is absent. Cannot be reached via normal analysis.
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let variant_def = DefId(1);
    checker.variant_parent.insert(variant_def, DefId(0));

    let mut callee = HirExpression {
        id: radix_hir::HirId(0),
        kind: HirExpressionKind::Path(variant_def),
        ty: None,
        span: test_span(),
    };
    let mut args = vec![HirCallArg {
        name: None,
        spread: false,
        marker: None,
        expr: literal_int(1, 1),
        span: test_span(),
    }];

    let result = checker.check_call(&mut callee, &[], &mut args, None);

    assert_eq!(result, checker.error_type);
    assert!(checker.errors.iter().any(|err| {
        err.kind == SemanticErrorKind::VariableUndefined
            && err
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "unknown_variant_fields")
            && args_have_no_legacy_message(&err.args)
    }));
}

#[test]
fn reports_unknown_variant_pattern_with_structured_event() {
    // Calls checker.check_pattern() directly with a fake DefId to test
    // the unknown-variant diagnostic path.
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let expected = checker.numerus_type();
    let pattern = HirPattern::Variant(DefId(404), Vec::new());

    checker.check_pattern(&pattern, expected, test_span());

    assert!(checker.errors.iter().any(|err| {
        err.kind == SemanticErrorKind::VariableUndefined
            && err
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "unknown_variant")
            && args_have_no_legacy_message(&err.args)
    }));
}

#[test]
fn literal_pattern_type_mismatch_uses_structured_issue_arg() {
    // Calls checker.check_pattern() directly to test pattern-vs-expected-type matching.
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);
    let expected = checker.numerus_type();
    let pattern = HirPattern::Literal(HirLiteral::Bool(true));

    checker.check_pattern(&pattern, expected, test_span());

    assert!(checker.errors.iter().any(|err| {
        err.kind == SemanticErrorKind::TypeMismatch
            && test_support::semantic_error_has_issue(err, "pattern_type_mismatch")
    }));
}

// ===========================================================================
// Tests already using source strings (unchanged from original)
// ===========================================================================

#[test]
fn structured_cape_consumes_local_iace_without_function_alternate_exit() {
    let result = crate::test_support::analyze_source(
        r#"functio handled() → numerus { fac { iace "bad" } cape err { redde 0 } redde 1 }"#,
    );

    if result.is_err() {
        let errors = result.diagnostics.clone();
        panic!("expected local cape to consume iace: {errors:?}");
    }
}

#[test]
fn gpu_builtin_call_rejects_non_device_function() {
    let result = crate::test_support::analyze_source(
        r"functio host() → u32 { redde global_invocation_x() }",
    );
    assert!(!result.is_ok(), "host builtin call should reject");
    let errors = &result.diagnostics;

    assert!(
        errors.iter().any(|err| err.args
            == vec![radix_diagnostics::DiagnosticArg::new(
                "issue",
                "gpu_builtin_device_function_required"
            )]),
        "{errors:?}"
    );
}

#[test]
fn gpu_builtin_call_typechecks_inside_device_function_but_is_a_raw_lane_id() {
    let result = crate::test_support::analyze_source(
        r"
@ nucleum fragment
functio helper() → u32 {
    redde workgroup_size_y()
}
",
    );

    // The call is well typed; kernel legality (gpu-reset rule 12.5) is the
    // only pass that rejects it.
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|err| err.is_error())
        .collect();
    assert!(
        !errors.is_empty()
            && errors
                .iter()
                .all(|err| err.issue() == Some("nucleum_raw_lane_id")),
        "expected only nucleum_raw_lane_id: {errors:?}"
    );
}

#[test]
fn structured_cape_consumes_failable_direct_call() {
    let result = crate::test_support::analyze_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }
functio handled() → numerus { fac { redde fail() } cape err { redde 0 } }
"#,
    );

    if result.is_err() {
        let errors = result.diagnostics.clone();
        panic!("expected local cape to consume failable call: {errors:?}");
    }
}

#[test]
fn structured_cape_rejects_incompatible_handler_error_types() {
    let result = crate::test_support::analyze_source(
        r#"
functio fail_text() → numerus ⇥ textus { iace "bad" }
functio fail_num() → numerus ⇥ numerus { iace 1 }
functio handled() → numerus {
    fac {
        fail_text()
        fail_num()
    } cape err {
        redde 0
    }
}
"#,
    );

    assert!(
        !result.is_ok(),
        "expected mixed handled error types to be rejected"
    );
    let errors = &result.diagnostics;
    assert!(errors.iter().any(|err| err.args
        == vec![radix_diagnostics::DiagnosticArg::new(
            "issue",
            "failable_call_handler_error_type_mismatch"
        )]));
}

#[test]
fn failable_call_rejects_enclosing_error_type_mismatch() {
    let result = crate::test_support::analyze_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }
functio caller() → numerus ⇥ numerus {
    redde fail()
}
"#,
    );

    assert!(
        result.is_err(),
        "expected failable call error type mismatch to be rejected"
    );
    let errors = &result.diagnostics;
    // The declared-error-conversion law (error-conversion-declarations,
    // archived 47ecd455b) names the rejection after the missing conversion
    // arm and carries the error-type pair it could not convert.
    assert!(errors.iter().any(|err| err.args
        == vec![
            radix_diagnostics::DiagnosticArg::new("issue", "failable_call_conversion_absent"),
            radix_diagnostics::DiagnosticArg::new("source", "textus"),
            radix_diagnostics::DiagnosticArg::new("destination", "numerus"),
        ]));
}

#[test]
fn rejects_summa_on_non_numeric_lista_elements() {
    let result = crate::test_support::analyze_source(
        r#"
incipit {
    sit _ names ← ["Aulus", "Balbus"] ∷ lista<textus>
    nota names.summa()
}
"#,
    );

    assert!(
        !result.is_ok(),
        "summa should require numeric lista elements"
    );
    let errors = &result.diagnostics;
    assert!(errors.iter().any(|err| {
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "numeric_lista_elements_required")
    }));
}

#[test]
fn literal_patterns_use_scrutinee_integer_width() {
    let result = crate::test_support::analyze_source(
        r"
functio choose(u32 id) → u32 {
    elige id {
        casu 0 {
            redde id
        }
        ceterum {
            redde id
        }
    }
}
",
    );

    if result.is_err() {
        let diagnostics = result.diagnostics.clone();
        panic!("expected literal pattern to inherit u32 scrutinee type: {diagnostics:?}");
    }
}

#[test]
fn failable_call_propagates_via_redde_in_failable_function() {
    let result = crate::test_support::analyze_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }
functio caller() → numerus ⇥ textus { redde fail() }
"#,
    );

    if result.is_err() {
        let errors = result.diagnostics.clone();
        panic!("expected `redde fail()` to propagate in a ⇥ function: {errors:?}");
    }
}

#[test]
fn failable_call_propagates_when_binding_in_failable_function() {
    let result = crate::test_support::analyze_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }
functio caller() → numerus ⇥ textus {
    sit numerus x ← fail()
    redde x
}
"#,
    );

    if result.is_err() {
        let errors = result.diagnostics.clone();
        panic!("expected failable call binding to propagate in a ⇥ function: {errors:?}");
    }
}

#[test]
fn failable_call_propagates_as_operand_in_failable_function() {
    let result = crate::test_support::analyze_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }
functio caller() → numerus ⇥ textus { redde fail() + 1 }
"#,
    );

    if result.is_err() {
        let errors = result.diagnostics.clone();
        panic!("expected failable call operand to propagate in a ⇥ function: {errors:?}");
    }
}

#[test]
fn failable_call_propagates_as_statement_in_failable_function() {
    let result = crate::test_support::analyze_source(
        r#"
functio fail() → vacuum ⇥ textus { iace "bad" }
functio caller() → vacuum ⇥ textus {
    fail()
}
"#,
    );

    if result.is_err() {
        let errors = result.diagnostics.clone();
        panic!("expected failable call statement to propagate in a ⇥ function: {errors:?}");
    }
}

#[test]
fn failable_call_still_rejected_in_closure_without_alternate_exit() {
    let result = crate::test_support::analyze_source(
        r#"
functio fail() → numerus ⇥ textus { iace "bad" }
functio caller() → numerus ⇥ textus {
    sit _ thunk ← () → numerus ∴ fail()
    redde thunk()
}
"#,
    );

    assert!(
        !result.is_ok(),
        "expected failable call in infallible closure to be rejected"
    );
    let errors = &result.diagnostics;
    assert!(errors.iter().any(|err| err.args
        == vec![radix_diagnostics::DiagnosticArg::new(
            "issue",
            "failable_call_requires_handler"
        )]));
}

// ===========================================================================
// Stage 1G: iuncta tuple type-checking
// ===========================================================================

#[test]
fn iuncta_tuple_construction_typechecks() {
    let source = r"
functio make_pair(f32 a, f32 b) → iuncta<f32, f32> {
    redde iuncta<f32, f32> [a, b]
}
";
    let _result = analyze(source);
}

#[test]
fn iuncta_tuple_construction_rejects_wrong_arity() {
    let result = crate::test_support::analyze_source(
        r"
functio make_pair(f32 a, f32 b) → iuncta<f32, f32> {
    redde iuncta<f32, f32> [a, b, a]
}
",
    );
    assert!(!result.is_ok(), "expected rejection, got success");
    let diagnostics = result.diagnostics.clone();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.issue() == Some("iuncta_construction_arity_mismatch")),
        "expected iuncta_construction_arity_mismatch, got: {diagnostics:?}",
    );
}

#[test]
fn iuncta_tuple_index_access_typechecks() {
    let source = r"
functio first(f32 a, f32 b) → f32 {
    sit iuncta<f32, f32> pair ← iuncta<f32, f32> [a, b]
    redde pair[0]
}
";
    let _result = analyze(source);
}

#[test]
fn iuncta_tuple_index_out_of_bounds() {
    let result = crate::test_support::analyze_source(
        r"
functio bad(f32 a, f32 b) → f32 {
    sit iuncta<f32, f32> pair ← iuncta<f32, f32> [a, b]
    redde pair[5]
}
",
    );
    assert!(!result.is_ok(), "expected rejection, got success");
    let diagnostics = result.diagnostics.clone();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.issue() == Some("tuple_index_out_of_bounds")),
        "expected tuple_index_out_of_bounds, got: {diagnostics:?}",
    );
}

#[test]
fn iuncta_tuple_destructuring_typechecks() {
    let source = r"
functio sum_pair(iuncta<f32, f32> pair) → f32 {
    sit [x, y] ← pair
    redde x + y
}
";
    test_support::assert_accepts(source);
}

#[test]
fn iuncta_tuple_array_destructuring_width_mismatch_uses_binding_issue() {
    test_support::assert_rejects_with_issue(
        r"
functio bad(iuncta<f32, f32> pair) → f32 {
    sit [x, y, z] ← pair
    redde x
}
",
        "tuple_index_out_of_bounds",
    );
}

#[test]
fn iuncta_tuple_variable_index_rejected() {
    let result = crate::test_support::analyze_source(
        r"
functio bad(numerus i, iuncta<f32, f32> pair) → f32 {
    redde pair[i]
}
",
    );
    assert!(!result.is_ok(), "expected rejection, got success");
    let diagnostics = result.diagnostics.clone();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.issue() == Some("tuple_index_must_be_constant")),
        "expected tuple_index_must_be_constant, got: {diagnostics:?}",
    );
}

// F1 regression: explicit iuncta type args preserved when no contextual expected.
#[test]
fn iuncta_explicit_type_args_enforce_element_types() {
    // With the F1 fix, explicit type args from `iuncta<numerus, numerus>` are
    // preserved through HIR lowering and used as element expected types when no
    // contextual expected tuple exists. Without the fix, element types are
    // synthesized from the literal expressions and the explicit type args are
    // silently dropped. The test below uses `ignotum` as the return type so
    // there is no contextual expected tuple — but the explicit type args in the
    // constructor should still be honoured and enforce element type checking.
    let result = crate::test_support::analyze_source(
        r#"
functio bad() → ignotum {
    redde iuncta<numerus, numerus> [1, "hello"]
}
"#,
    );
    assert!(!result.is_ok(), "expected rejection, got success");
    let diagnostics = result.diagnostics.clone();
    assert!(
        diagnostics
            .iter()
            .any(|d| { d.issue() == Some("expression_type_mismatch") }),
        "expected expression_type_mismatch, got: {diagnostics:?}",
    );
}

// F1 regression: explicit iuncta type args work in position without contextual expected.
#[test]
fn iuncta_explicit_type_args_accepted_with_ignotum_return() {
    // Explicit type args match element types — should typecheck cleanly.
    let source = r"
functio make() → ignotum {
    redde iuncta<numerus, numerus> [1, 2]
}
";
    let _result = analyze(source);
}

// F2 regression: Option<iuncta<...>> is assignable to ignotum sink.
#[test]
fn option_iuncta_assignable_to_ignotum() {
    // Before the F2 fix, the Option→Tuple rejection fired before the ignotum
    // catch-all, so `Option<iuncta<f32, f32>>` → `ignotum` was incorrectly
    // rejected. `make_opt_pair` returns `iuncta<f32, f32> ∪ nihil` which
    // lowers to `Option<iuncta<f32, f32>>`. Assigning that to ignotum must
    // succeed.
    let source = r"
functio make_opt_pair() → iuncta<f32, f32> ∪ nihil {
    redde iuncta<f32, f32> [1.0, 2.0]
}

functio sink() → ignotum {
    redde make_opt_pair()
}
";
    let _result = analyze(source);
}

// ---------------------------------------------------------------------------
// PRP-015: occurs_in must check every TypeId-bearing composite variant.
// Before the fix, 9 variants fell through to `_ => false`, allowing recursive
// inference substitutions to go undetected.
// ---------------------------------------------------------------------------

#[test]
fn occurs_in_catches_all_composite_variants() {
    use crate::InferVar;
    use handroll::FxHashMap;
    use radix_lexer::Symbol;
    use radix_types::IndexExpr;

    let mut types = TypeTable::new();
    let resolver = Resolver::new();

    // Pre-allocate shape indices that don't need the inference variable.
    let numerus = types.primitive(Primitive::Numerus);
    let shape = types.intern_index(IndexExpr::Literal(1));

    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);

    // Allocate one inference variable: InferVar(0) → Type::Infer(InferVar(0)).
    let infer_ty = checker.fresh_infer();
    let var = InferVar(0);

    // Negative control: occurs_in on a type WITHOUT the InferVar returns false.
    assert!(
        !checker.occurs_in(var, numerus),
        "primitive must not contain infer var"
    );

    // Each TypeId-bearing composite variant that was previously skipped by
    // `_ => false` must now recurse into its children. Allocate through
    // checker.types because the checker holds the mutable TypeTable borrow.
    let tuple_ty = checker.types.tuple(vec![infer_ty]);
    assert!(checker.occurs_in(var, tuple_ty), "Tuple must check occurs");

    let mut fields = FxHashMap::default();
    fields.insert(Symbol(0), infer_ty);
    let record_ty = checker.types.record(fields);
    assert!(
        checker.occurs_in(var, record_ty),
        "Record must check occurs"
    );

    let tensor_ty = checker.types.tensor_with_shape(infer_ty, shape);
    assert!(
        checker.occurs_in(var, tensor_ty),
        "Tensor must check occurs"
    );

    let vector_ty = checker.types.vector_with_width(infer_ty, shape);
    assert!(
        checker.occurs_in(var, vector_ty),
        "Vector must check occurs"
    );

    let matrix_ty = checker.types.matrix_with_shape(infer_ty, shape);
    assert!(
        checker.occurs_in(var, matrix_ty),
        "Matrix must check occurs"
    );

    let sparsa_ty = checker.types.sparsa_with_shape(infer_ty, shape);
    assert!(
        checker.occurs_in(var, sparsa_ty),
        "Sparsa must check occurs"
    );

    let atomic_ty = checker.types.atomic(infer_ty);
    assert!(
        checker.occurs_in(var, atomic_ty),
        "Atomic must check occurs"
    );

    let intervallum_ty = checker.types.intervallum(infer_ty);
    assert!(
        checker.occurs_in(var, intervallum_ty),
        "Intervallum must check occurs"
    );

    // Alias(DefId, TypeId) — the most dangerous omission: T = Alias(d, T)
    let alias_ty = checker.types.intern(Type::Alias(DefId(1), infer_ty));
    assert!(checker.occurs_in(var, alias_ty), "Alias must check occurs");
}

// ---------------------------------------------------------------------------
// em-u4: errored_exprs dedup regression (error-masking final unit).
//
// The typechecker's `errored_exprs: FxHashSet<HirId>` guard (typecheck/mod.rs)
// makes the `HirExpressionKind::Error` placeholder branch report
// `invalid_lowered_expression` exactly once per error node id (expr.rs). These
// tests lock that invariant against cascade/double-report: the same lowered
// error placeholder reached from two parent positions must fire once, and two
// distinct root causes must each fire once (dedup by node id, not by kind).
// ---------------------------------------------------------------------------

/// Count `invalid_lowered_expression` errors the checker reported.
fn count_invalid_lowered_expression(checker: &TypeChecker) -> usize {
    checker
        .errors
        .iter()
        .filter(|err| {
            err.kind == SemanticErrorKind::LoweringError
                && test_support::semantic_error_has_issue(err, "invalid_lowered_expression")
        })
        .count()
}

#[test]
fn error_placeholder_reports_once_per_node_id() {
    // A lowering error placeholder (`HirExpressionKind::Error`) reached twice —
    // the same node id referenced from two parent positions, the shared-node
    // shape a record-and-continue lowerer can produce — must report
    // `invalid_lowered_expression` exactly once. Without the `errored_exprs`
    // guard this fires once per visit and cascades duplicate diagnostics.
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);

    let shared_error = HirExpression {
        id: radix_hir::HirId(7),
        kind: HirExpressionKind::Error,
        ty: None,
        span: test_span(),
    };
    let mut parent_a = shared_error.clone();
    let mut parent_b = shared_error.clone();

    checker.check_expr(&mut parent_a);
    checker.check_expr(&mut parent_b);

    assert_eq!(
        count_invalid_lowered_expression(&checker),
        1,
        "the same error node visited twice must report exactly once, got: {:?}",
        checker.errors
    );
}

#[test]
fn distinct_error_placeholders_each_report_once() {
    // Per-root-cause granularity: two DISTINCT error placeholder nodes (two
    // independent lowered errors) each report exactly once — the guard dedups
    // by node id and must not swallow independent root causes.
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, None, Target::HirRust);

    let mut err_a = HirExpression {
        id: radix_hir::HirId(1),
        kind: HirExpressionKind::Error,
        ty: None,
        span: test_span(),
    };
    let mut err_b = HirExpression {
        id: radix_hir::HirId(2),
        kind: HirExpressionKind::Error,
        ty: None,
        span: test_span(),
    };

    checker.check_expr(&mut err_a);
    checker.check_expr(&mut err_b);

    assert_eq!(
        count_invalid_lowered_expression(&checker),
        2,
        "two independent error nodes must report exactly once each, got: {:?}",
        checker.errors
    );
}

#[test]
fn unmarked_capturing_closure_behavior_is_unchanged() {
    let source = r"
incipit {
    sit numerus seed ← 2
    sit _ closure ← (numerus value) ∴ value + seed
}
";
    let unit = analyze(source);
    assert!(
        unit.is_ok(),
        "an unmarked closure may capture enclosing locals: {:?}",
        unit.diagnostics
    );
}

#[test]
fn ordinary_closure_error_channel_behavior_is_unchanged() {
    let source = r#"
functio guard(numerus value) → numerus ⇥ textus {
    redde value
}
functio take() → vacuum {
    sit _ plain ← clausura numerus value → numerus ⇥ textus {
        si value ≻ 3 {
            iace "too big"
        }
        redde value
    }
    tacet
}
"#;
    let unit = analyze(source);
    assert!(
        unit.is_ok(),
        "an unmarked closure keeps its explicit ⇥ channel: {:?}",
        unit.diagnostics
    );
}

// =============================================================================
// EOT-1 — typecheck red drivers and unchanged legacy forms
// =============================================================================

#[test]
fn eot_selective_imported_callable_alias_typechecks_red_driver() {
    let result = analyze(
        r#"importa ex "norma:consolum" fixum dic ut output, funde ut output_bytes,
incipit { output("salve") }"#,
    );
    assert!(
        result.is_ok(),
        "EOT-2M red driver: imported callable aliases must typecheck as ordinary values; current diagnostics={:?}; expected later issues include `selective_import_missing_member`, and `selective_import_duplicate_alias`",
        result.diagnostics
    );
}

#[test]
fn eot_legacy_output_and_input_forms_typecheck_unchanged() {
    let result = analyze(
        r#"incipit {
  nota "legacy text"
  mone "legacy warning"
  vide "legacy debug"
  scribe "legacy compatibility"
  sit _ input ← lege
  nota input
}"#,
    );
    assert!(
        result.is_ok(),
        "legacy output/input baseline must remain typecheckable: {:?}",
        result.diagnostics
    );
}

// =============================================================================
// EOT-2M — selective member resolution through the imported file interface
// =============================================================================

fn eot_selective_test_interface() -> crate::file_interface::FileInterface {
    use crate::file_interface::{
        FileExport, FileExportKind, InterfaceCallable, InterfaceParam, InterfaceStructExport,
        InterfaceTypeSnapshot,
    };
    use crate::{Primitive, SemanticParamMode};

    let callable = || InterfaceCallable {
        type_params: Vec::new(),
        params: vec![InterfaceParam {
            ty: InterfaceTypeSnapshot::Primitive(Primitive::Textus),
            mode: SemanticParamMode::Ref,
            optional: false,
            sponte: false,
            axis: false,
        }],
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
        err: None,
        is_async: false,
        is_generator: false,
        tensor_edge_policy_requirements: Vec::new(),
        device: None,
    };
    let mut interface = crate::file_interface::FileInterface::new();
    interface.insert(FileExport {
        name: "dic".to_owned(),
        kind: FileExportKind::Function(callable()),
    });
    interface.insert(FileExport {
        name: "PI".to_owned(),
        kind: FileExportKind::Const(InterfaceTypeSnapshot::Primitive(Primitive::Fractus), None),
    });
    interface.insert(FileExport {
        name: "Vector".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: "Vector".to_owned(),
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    interface
}

fn eot_analyze_with_selective_interface(source: &str) -> test_support::AnalyzedFixture {
    test_support::analyze_source_with_import_contract(
        source,
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::from([(
            "norma:consolum".to_owned(),
            eot_selective_test_interface(),
        )]),
    )
}

#[test]
fn selective_import_missing_member_reports_named_issue() {
    let result = eot_analyze_with_selective_interface(
        r#"importa ex "norma:consolum" fixum absens,
incipit { tacet }"#,
    );
    assert!(result.is_err());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| test_support::issue(err) == Some("selective_import_missing_member")),
        "expected selective_import_missing_member, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn selective_import_type_export_binds_a_type() {
    // D22.5: selective imports accept types as well as values.
    let result = eot_analyze_with_selective_interface(
        r#"importa ex "norma:consolum" fixum Vector
functio f(Vector v) → Vector { redde v }
incipit { tacet }"#,
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::SemanticError::is_error),
        "selective type import must bind the type: {:?}",
        result.diagnostics
    );
}

/// Need: the imported snapshot carries `sponte` apart from `optional`.
fn imported_omittable_param_interface(sponte: bool) -> crate::file_interface::FileInterface {
    use crate::file_interface::{
        FileExport, FileExportKind, InterfaceCallable, InterfaceParam, InterfaceTypeSnapshot,
    };
    use crate::{Primitive, SemanticParamMode};

    let mut interface = crate::file_interface::FileInterface::new();
    interface.insert(FileExport {
        name: "prende".to_owned(),
        kind: FileExportKind::Function(InterfaceCallable {
            type_params: Vec::new(),
            params: vec![InterfaceParam {
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                mode: SemanticParamMode::Owned,
                optional: true,
                sponte,
                axis: false,
            }],
            ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
            err: None,
            is_async: false,
            is_generator: false,
            tensor_edge_policy_requirements: Vec::new(),
            device: None,
        }),
    });
    interface
}

fn analyze_optional_argument_into_imported_param(sponte: bool) -> test_support::AnalyzedFixture {
    test_support::analyze_source_with_import_contract(
        r#"importa ex "norma:consolum" fixum prende
functio g(numerus ∪ nihil x) { prende(x) }
incipit { tacet }"#,
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::from([(
            "norma:consolum".to_owned(),
            imported_omittable_param_interface(sponte),
        )]),
    )
}

#[test]
fn optional_argument_into_imported_defaulted_non_sponte_param_is_sem010() {
    let result = analyze_optional_argument_into_imported_param(false);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error()
                && test_support::issue(err) == Some("optional_into_plain_cell")),
        "expected optional_into_plain_cell, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn optional_argument_into_imported_sponte_param_is_accepted() {
    let result = analyze_optional_argument_into_imported_param(true);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn selective_import_function_member_types_call_through_alias() {
    let ok = eot_analyze_with_selective_interface(
        r#"importa ex "norma:consolum" fixum dic ut output,
incipit { output("salve") }"#,
    );
    assert!(
        ok.is_ok(),
        "interface-supplied signature must typecheck the aliased call: {:?}",
        ok.diagnostics
    );

    // The installed signature governs the call site: wrong arity fails.
    let bad = eot_analyze_with_selective_interface(
        r#"importa ex "norma:consolum" fixum dic ut output,
incipit { output() }"#,
    );
    assert!(
        bad.is_err(),
        "wrong-arity call through the alias must fail against the installed signature"
    );

    // The complete function value type sits on the caller-owned local DefId.
    let mut ok = ok;
    let output_name = ok.interner.intern("output");
    let def_id = ok
        .resolver
        .lookup(output_name)
        .expect("alias binding DefId");
    let symbol = ok.resolver.get_symbol(def_id).expect("alias symbol");
    let ty = symbol.ty.expect("alias must carry the function value type");
    assert!(
        matches!(ok.types.get(ty), crate::Type::Func(_)),
        "alias type must be the complete function value type"
    );
}

#[test]
fn selective_import_const_member_carries_interface_type() {
    let mut result = eot_analyze_with_selective_interface(
        r#"importa ex "norma:consolum" fixum PI ut pi,
incipit { tacet }"#,
    );
    assert!(
        result.is_ok(),
        "const member must bind with its interface type: {:?}",
        result.diagnostics
    );
    let pi_name = result.interner.intern("pi");
    let def_id = result
        .resolver
        .lookup(pi_name)
        .expect("const alias binding DefId");
    let symbol = result
        .resolver
        .get_symbol(def_id)
        .expect("const alias symbol");
    assert_eq!(
        symbol.ty,
        Some(result.types.primitive(crate::Primitive::Fractus)),
        "const alias must carry the interface-supplied value type"
    );
}

#[test]
fn selective_import_hir_carries_ordered_members_and_marker() {
    use radix_hir::{HirItemKind, HirModule};

    let mut result = eot_analyze_with_selective_interface(
        r#"importa ex "norma:consolum" fixum dic ut output, PI,
incipit { tacet }"#,
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
    let (dic, output, pi) = {
        let interner = &mut result.interner;
        (
            interner.intern("dic"),
            interner.intern("output"),
            interner.intern("PI"),
        )
    };
    let hir: &HirModule = result.hir.as_ref().expect("hir");
    let imports: Vec<_> = hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Import(import) => Some(import),
            _ => None,
        })
        .collect();
    assert_eq!(imports.len(), 1, "one selective import item");
    let import = imports[0];
    assert!(import.selective, "selective marker must be set");
    let names: Vec<_> = import
        .items
        .iter()
        .map(|item| (item.name, item.alias))
        .collect();
    assert_eq!(
        names,
        vec![(dic, Some(output)), (pi, None)],
        "ordered exported spellings with caller-owned aliases"
    );

    // Legacy namespace imports keep the unmarked shape.
    let legacy = test_support::analyze_source(
        r#"importa ex "norma:consolum" consolum
incipit { tacet }"#,
    );
    assert!(legacy.is_ok(), "diagnostics: {:?}", legacy.diagnostics);
    let legacy_hir = legacy.hir.as_ref().expect("hir");
    let legacy_import = legacy_hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Import(import) => Some(import),
            _ => None,
        })
        .expect("legacy import item");
    assert!(!legacy_import.selective, "namespace import stays unmarked");
}

// ===========================================================================
// Typed zero `Type {}`: every annotation spelling is accepted before `{}`
// ===========================================================================

#[test]
fn typed_zero_accepts_every_register_and_tensor_spelling() {
    let result = crate::test_support::analyze_source(
        r"
incipit {
    sit vf32[4] short_vector ← vf32[4] {}
    sit vector<f32, magnitudo 4> long_vector ← vector<f32, magnitudo 4> {}
    sit mf32[2, 2] short_matrix ← mf32[2, 2] {}
    sit matrix<f32, [2, 2]> long_matrix ← matrix<f32, [2, 2]> {}
    sit tf32[2, 3] short_tensor ← tf32[2, 3] {}
    sit tensor<f32, [2, 3]> long_tensor ← tensor<f32, [2, 3]> {}
}
",
    );
    assert!(
        result.is_ok(),
        "every typed-zero spelling must check: {:?}",
        result.diagnostics
    );
    let spelled = result
        .result
        .presentation
        .authored_types
        .iter()
        .filter(|authored| authored.site.slot == radix_hir::HirTypeSlot::TypedZero)
        .count();
    assert_eq!(spelled, 6, "each typed zero records its authored type");
}

#[test]
fn inferred_constructor_entries_build_expected_map_in_binding_return_and_argument() {
    test_support::assert_accepts(
        "
functio f() → tabula<textus, numerus> { redde _ { \"a\": 1, \"b\": 2 } }
functio g(tabula<textus, numerus> m) → numerus { redde 0 }
incipit {
    sit tabula<textus, numerus> m ← _ { \"a\": 1 }
    sit tabula<textus, numerus> n ← f()
    nota g(_ { \"a\": 3 })
}
",
    );
}

#[test]
fn inferred_constructor_empty_braces_build_empty_map() {
    test_support::assert_accepts("incipit { sit tabula<textus, numerus> m ← _ { } }");
}

#[test]
fn inferred_constructor_entries_value_type_mismatch_is_the_bare_literal_error() {
    test_support::assert_rejects_with_issue(
        "incipit { sit tabula<textus, numerus> m ← _ { \"a\": \"x\" } }",
        "json_value_type_mismatch",
    );
}

#[test]
fn inferred_constructor_entries_for_genus_is_named_error() {
    test_support::assert_rejects_with_issue(
        "genus P { varia numerus x = 0 } incipit { sit P p ← _ { \"x\": 1 } }",
        "inferred_constructor_entries_need_map",
    );
}

#[test]
fn inferred_constructor_fields_for_map_is_named_error() {
    test_support::assert_rejects_with_issue(
        "incipit { sit tabula<textus, numerus> m ← _ { x = 1 } }",
        "inferred_constructor_fields_for_map",
    );
}

#[test]
fn inferred_constructor_entries_without_expected_type_is_named_error() {
    test_support::assert_rejects_with_issue(
        "incipit { nota _ { \"a\": 1 } }",
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn inferred_constructor_entries_build_expected_json_value() {
    test_support::assert_accepts(
        "
functio f() → json { redde _ { \"a\": 1, \"b\": [1, 2] } }
functio g(json v) → numerus { redde 0 }
incipit {
    sit json j ← _ { \"a\": 1 }
    nota g(_ { \"k\": \"v\" })
    nota g(f())
}
",
    );
}

#[test]
fn inferred_constructor_with_hole_on_both_sides_is_named_error() {
    test_support::assert_rejects_with_issue(
        "genus P { varia numerus x = 0 } incipit { sit _ p ← _ { x = 1 } }",
        "inferred_constructor_no_expected_type",
    );
    test_support::assert_rejects_with_issue(
        "incipit { sit _ m ← _ { \"a\": 1 } }",
        "inferred_constructor_no_expected_type",
    );
    test_support::assert_rejects_with_issue(
        "incipit { sit _ z ← _ {} }",
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn inferred_constructor_against_optional_genus_builds_the_genus_in_binding_argument_and_return() {
    test_support::assert_accepts(
        "
genus P { varia numerus x = 0 }
functio f() → P ∪ nihil { redde _ { x = 1 } }
functio g(P ∪ nihil p) → numerus { redde 0 }
incipit {
    sit P ∪ nihil p ← _ { x = 2 }
    nota g(_ { x = 3 })
    nota g(f())
}
",
    );
}

#[test]
fn inferred_constructor_against_optional_map_builds_the_map() {
    test_support::assert_accepts(
        "
functio f() → tabula<textus, numerus> ∪ nihil { redde _ { \"a\": 1 } }
functio g(tabula<textus, numerus> ∪ nihil m) → numerus { redde 0 }
incipit {
    sit tabula<textus, numerus> ∪ nihil m ← _ { \"a\": 1 }
    sit tabula<textus, numerus> ∪ nihil e ← _ { }
    nota g(_ { \"a\": 2 })
    nota g(f())
}
",
    );
}

#[test]
fn inferred_constructor_against_union_of_several_types_is_named_error() {
    test_support::assert_rejects_with_issue(
        "genus P { varia numerus x = 0 } incipit { sit P ∪ textus ∪ nihil p ← _ { x = 1 } }",
        "inferred_constructor_ambiguous_union",
    );
    test_support::assert_rejects_with_issue(
        "genus P { varia numerus x = 0 } incipit { sit P ∪ textus p ← _ { x = 1 } }",
        "inferred_constructor_ambiguous_union",
    );
}

#[test]
fn inferred_bracket_literal_builds_expected_tuple_in_binding_return_and_argument() {
    test_support::assert_accepts(
        "
functio f() → iuncta<numerus, textus> { redde _ [1, \"a\"] }
functio g(iuncta<numerus, textus> t) → numerus { redde t[0] }
incipit {
    sit iuncta<numerus, textus> t ← _ [1, \"a\"]
    sit iuncta<numerus, numerus> v ← [5, 6]
    sit lista<numerus> xs ← _ [1, 2]
    nota g(_ [3, \"c\"])
    nota g([4, \"d\"])
}
",
    );
}

#[test]
fn inferred_bracket_literal_without_expected_type_is_named_error() {
    test_support::assert_rejects_with_issue(
        "incipit { nota _ [1, 2] }",
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn inferred_bracket_literal_with_wrong_tuple_arity_is_named_error() {
    test_support::assert_rejects_with_issue(
        "incipit { sit iuncta<numerus, numerus> t ← _ [1, 2, 3] }",
        "iuncta_construction_arity_mismatch",
    );
}

#[test]
fn bare_field_literal_takes_the_class_cell_type() {
    test_support::assert_accepts(
        "
genus P { varia numerus x = 0 varia numerus y = 0 }
functio f() → P { redde { x = 1, y = 2 } }
functio g(P p) → numerus { redde p.x }
incipit {
    sit P p ← { x = 1 }
    nota g({ x = 2, y = 3 })
    nota f().y
}
",
    );
}

#[test]
fn inferred_constructor_against_only_none_is_the_no_expected_type_error() {
    test_support::assert_rejects_with_issue(
        "incipit { sit nihil p ← _ { x = 1 } }",
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn inferred_constructor_wrong_shape_against_optional_member_is_the_existing_shape_error() {
    test_support::assert_rejects_with_issue(
        "genus P { varia numerus x = 0 } incipit { sit P ∪ nihil p ← _ { \"x\": 1 } }",
        "inferred_constructor_entries_need_map",
    );
    test_support::assert_rejects_with_issue(
        "incipit { sit tabula<textus, numerus> ∪ nihil m ← _ { x = 1 } }",
        "inferred_constructor_fields_for_map",
    );
}
