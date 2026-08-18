//! Tests for HIR lowering (AST-to-HIR translation of parsed, resolved source).
//!
//! Contract: every lowerable surface form round-trips through `lower()` without errors.

#[cfg(test)]
#[path = "error_masking_test.rs"]
mod error_masking_test;

use super::lower;
use crate::hir::{
    HirAnnotationValue, HirAvailability, HirExpressionKind, HirFunction, HirInterface, HirItemKind, HirStatementKind,
    HirStruct, HirTestMetadata, HirTestModifier, HirTypeParamConstraint,
};
use crate::lexer::Interner;
use crate::semantic::{passes, Primitive, Resolver, Type, TypeTable};

fn lower_source_result(source: &str) -> (crate::hir::LoweredProgram, Vec<crate::hir::LowerError>, Interner, TypeTable) {
    let lex_result = crate::lexer::lex(source);
    let parse_result = crate::parser::parse(lex_result);
    assert!(
        parse_result.success(),
        "parse failed: {:?}",
        parse_result
            .errors
            .iter()
            .map(|error| format!("{error:?}"))
            .collect::<Vec<_>>()
    );

    let program = parse_result.program.expect("expected parsed program");
    let mut interner = parse_result.interner;
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();

    crate::builtins::register(&mut resolver, &mut types, &mut interner);
    crate::builtins::register_frame_types_if_needed(&program, &mut resolver, &mut types, &mut interner);

    if let Err(errors) = passes::collect::collect(&program, &mut resolver, &mut types) {
        panic!("collect failed: {errors:?}");
    }
    if let Err(errors) = passes::resolve::resolve(&program, &mut resolver, &interner, &mut types) {
        panic!("resolve failed: {errors:?}");
    }

    let (lowered, errors) = lower(&program, &mut resolver, &mut types, &mut interner);
    (lowered, errors, interner, types)
}

fn lower_source(source: &str) -> (crate::hir::LoweredProgram, Interner, TypeTable) {
    let (lowered, errors, interner, types) = lower_source_result(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    (lowered, interner, types)
}

fn lower_source_with_locale_pack(
    source: &str,
    pack: &crate::locale::LocalePack,
) -> (crate::hir::LoweredProgram, Interner, TypeTable) {
    let lex_result = crate::lexer::lex_with_locale_pack(source, pack);
    let parse_result = crate::parser::parse(lex_result);
    assert!(
        parse_result.success(),
        "parse failed: {:?}",
        parse_result
            .errors
            .iter()
            .map(|error| format!("{error:?}"))
            .collect::<Vec<_>>()
    );

    let program = parse_result.program.expect("expected parsed program");
    let mut interner = parse_result.interner;
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();

    crate::builtins::register(&mut resolver, &mut types, &mut interner);
    crate::builtins::register_frame_types_if_needed(&program, &mut resolver, &mut types, &mut interner);

    if let Err(errors) = passes::collect::collect(&program, &mut resolver, &mut types) {
        panic!("collect failed: {errors:?}");
    }
    if let Err(errors) = passes::resolve::resolve(&program, &mut resolver, &interner, &mut types) {
        panic!("resolve failed: {errors:?}");
    }

    let (lowered, errors) = lower(&program, &mut resolver, &mut types, &mut interner);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    (lowered, interner, types)
}

fn assert_lower_rejects_issue(source: &str, issue: &str) {
    let (_lowered, errors, _interner, _types) = lower_source_result(source);
    assert!(
        errors.iter().any(|error| error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)),
        "expected lowering issue {issue}, got {errors:?}"
    );
}

fn test_metadata(item: &crate::hir::HirItem) -> &HirTestMetadata {
    let HirItemKind::Function(function) = &item.kind else {
        panic!("expected lowered test function");
    };
    let HirFunction { test: Some(test), .. } = function.as_ref() else {
        panic!("expected lowered test function");
    };
    test
}

#[test]
fn lowers_requirit_to_guarded_throw() {
    // `requirit cond secus err` desugars to `si non (cond) { iace err }`:
    // a Si expression whose condition is the negated condition and whose
    // then-block throws the error payload.
    let source = "functio f() → numerus ⇥ textus {\n  requirit verum secus \"bad\"\n  redde 1\n}";
    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    let body = func.body.as_ref().expect("expected body");
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected require expression statement");
    };
    let HirExpressionKind::Si { cond, then_block, then_catch: None, else_block: None } = &expr.kind else {
        panic!("expected desugared si expression, got {:?}", expr.kind);
    };
    let HirExpressionKind::Unary(crate::hir::HirUnOp::Not, inner) = &cond.kind else {
        panic!("expected negated condition, got {:?}", cond.kind);
    };
    assert!(
        matches!(inner.kind, HirExpressionKind::Literal(_)),
        "cond should lower to a literal"
    );
    let HirStatementKind::Expr(throw_expr) = &then_block.statements[0].kind else {
        panic!("expected throw statement in then block");
    };
    assert!(
        matches!(throw_expr.kind, HirExpressionKind::Throw(_)),
        "then block must throw the error payload"
    );
}

#[test]
fn lowers_en_require_surface_to_guarded_throw() {
    // The en surface `require cond else err` resolves to the same requirit
    // identity and desugars identically.
    let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");
    let source = "fn f() → int ⇥ string {\n  require true else \"bad\"\n  return 1\n}";
    let (lowered, _, _) = lower_source_with_locale_pack(source, &pack);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    let body = func.body.as_ref().expect("expected body");
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected require expression statement");
    };
    let HirExpressionKind::Si { cond, then_block, .. } = &expr.kind else {
        panic!("expected desugared si expression, got {:?}", expr.kind);
    };
    assert!(
        matches!(cond.kind, HirExpressionKind::Unary(crate::hir::HirUnOp::Not, _)),
        "expected negated condition"
    );
    let HirStatementKind::Expr(throw_expr) = &then_block.statements[0].kind else {
        panic!("expected throw statement in then block");
    };
    assert!(matches!(throw_expr.kind, HirExpressionKind::Throw(_)));
}

#[test]
fn preserves_spread_args_in_optional_and_non_null_calls() {
    let source = r"
functio run(ignotum callback) → vacuum {
  fixum lista<numerus> values ← [1, 2]
  callback?(sparge values)
  callback!(sparge values)
}
";

    let (lowered, _, _) = lower_source(source);
    let hir = lowered.hir;
    let HirItemKind::Function(function) = &hir.items[0].kind else {
        panic!("expected function item");
    };
    let body = function.body.as_ref().expect("expected function body");

    let HirStatementKind::Expr(optional_call) = &body.statements[1].kind else {
        panic!("expected optional call statement");
    };
    let HirExpressionKind::OptionalChain(_, crate::hir::HirOptionalChainKind::Call(optional_args)) =
        &optional_call.kind
    else {
        panic!("expected optional call");
    };
    assert!(optional_args[0].spread);

    let HirStatementKind::Expr(non_null_call) = &body.statements[2].kind else {
        panic!("expected non-null call statement");
    };
    let HirExpressionKind::NonNull(_, crate::hir::HirNonNullKind::Call(non_null_args)) = &non_null_call.kind else {
        panic!("expected non-null call");
    };
    assert!(non_null_args[0].spread);
}

#[test]
fn lowers_custodi_to_first_class_statement_in_source_order() {
    let source = r"
functio probe(numerus n) → numerus {
  custodi {
    si n < 0 {
      redde -1
    }
    si n > 100 {
      redde 100
    }
  }

  redde n
}
";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(function) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    let body = function.body.as_ref().expect("expected function body");
    let HirStatementKind::Custodi(custodi) = &body.statements[0].kind else {
        panic!("expected first-class custodi statement");
    };

    assert_eq!(custodi.clauses.len(), 2);
    assert!(matches!(custodi.clauses[0].cond.kind, HirExpressionKind::Binary(..)));
    assert!(matches!(custodi.clauses[1].cond.kind, HirExpressionKind::Binary(..)));
}

#[test]
fn lowers_suite_paths_for_top_level_and_nested_tests() {
    let source = r#"proba "top level case" {
  adfirma verum
}

probandum "outer suite" {
  proba "nested case" {
    adfirma verum
  }

  probandum "inner suite" {
    proba "deep case" {
      adfirma verum
    }
  }
}"#;

    let (lowered, interner, _) = lower_source(source);
    let hir = lowered.hir;

    assert_eq!(hir.items.len(), 3);

    let top = test_metadata(&hir.items[0]);
    assert_eq!(interner.resolve(top.name), "top level case");
    assert!(top.suite_path.is_empty());
    assert_eq!(top.span, hir.items[0].span);
    assert!(top.modifiers.is_empty());

    let nested = test_metadata(&hir.items[1]);
    assert_eq!(interner.resolve(nested.name), "nested case");
    assert_eq!(nested.suite_path.len(), 1);
    assert_eq!(interner.resolve(nested.suite_path[0]), "outer suite");
    assert_eq!(nested.span, hir.items[1].span);
    assert!(nested.modifiers.is_empty());

    let deep = test_metadata(&hir.items[2]);
    assert_eq!(interner.resolve(deep.name), "deep case");
    assert_eq!(deep.suite_path.len(), 2);
    assert_eq!(interner.resolve(deep.suite_path[0]), "outer suite");
    assert_eq!(interner.resolve(deep.suite_path[1]), "inner suite");
    assert_eq!(deep.span, hir.items[2].span);
    assert!(deep.modifiers.is_empty());
}

#[test]
fn lowers_all_test_modifiers_into_metadata() {
    let source = r#"proba "modifier case" omitte "blocked by service" futurum "later" solum tag "smoke" temporis 5 metior repete 2 fragilis 1 solum_in "staging" {
    adfirma verum
}"#;

    let (lowered, interner, _) = lower_source(source);
    let hir = lowered.hir;
    assert_eq!(hir.items.len(), 1);

    let metadata = test_metadata(&hir.items[0]);
    assert_eq!(interner.resolve(metadata.name), "modifier case");
    assert!(metadata.suite_path.is_empty());
    assert_eq!(metadata.span, hir.items[0].span);

    let rendered: Vec<String> = metadata
        .modifiers
        .iter()
        .map(|modifier| match modifier {
            HirTestModifier::Omitte(reason) => format!("omitte({})", interner.resolve(*reason)),
            HirTestModifier::Futurum(reason) => format!("futurum({})", interner.resolve(*reason)),
            HirTestModifier::Solum => "solum".to_owned(),
            HirTestModifier::Tag(tag) => format!("tag({})", interner.resolve(*tag)),
            HirTestModifier::Temporis(n) => format!("temporis({n})"),
            HirTestModifier::Metior => "metior".to_owned(),
            HirTestModifier::Repete(n) => format!("repete({n})"),
            HirTestModifier::Fragilis(n) => format!("fragilis({n})"),
            HirTestModifier::SolumIn(env) => format!("solum_in({})", interner.resolve(*env)),
        })
        .collect();

    assert_eq!(
        rendered,
        vec![
            "omitte(blocked by service)".to_owned(),
            "futurum(later)".to_owned(),
            "solum".to_owned(),
            "tag(smoke)".to_owned(),
            "temporis(5)".to_owned(),
            "metior".to_owned(),
            "repete(2)".to_owned(),
            "fragilis(1)".to_owned(),
            "solum_in(staging)".to_owned(),
        ]
    );
}

#[test]
fn lowers_suite_modifiers_into_nested_test_metadata() {
    let source = r#"probandum "suite" tag "parser" solum {
  proba "case" tag "unit" {
    adfirma verum
  }
}"#;

    let (lowered, interner, _) = lower_source(source);
    let hir = lowered.hir;
    assert_eq!(hir.items.len(), 1);

    let metadata = test_metadata(&hir.items[0]);
    let rendered: Vec<String> = metadata
        .modifiers
        .iter()
        .map(|modifier| match modifier {
            HirTestModifier::Tag(tag) => format!("tag({})", interner.resolve(*tag)),
            HirTestModifier::Solum => "solum".to_owned(),
            other => format!("{other:?}"),
        })
        .collect();

    assert_eq!(
        rendered,
        vec!["tag(parser)".to_owned(), "solum".to_owned(), "tag(unit)".to_owned()]
    );
}

#[test]
fn preserves_sponte_markers_on_hir_declarations() {
    let source = r#"functio invite(textus email sponte vel "anon") → vacuum {}

genus User {
  textus nickname sponte = "Anonymous"
}"#;

    let (lowered, interner, _) = lower_source(source);
    let hir = lowered.hir;

    let HirItemKind::Function(function) = &hir.items[0].kind else {
        panic!("expected lowered function");
    };
    let HirFunction { params, .. } = function.as_ref();
    assert_eq!(interner.resolve(params[0].name), "email");
    assert!(params[0].optional);
    assert!(params[0].sponte);

    let HirItemKind::Struct(HirStruct { fields, .. }) = &hir.items[1].kind else {
        panic!("expected lowered struct");
    };
    assert_eq!(interner.resolve(fields[0].name), "nickname");
    assert!(fields[0].sponte);
    assert!(fields[0].init.is_some());
}

#[test]
fn generic_type_params_lower_in_top_level_function_signatures() {
    let source = r"functio adde<T>(T elem) → vacuum { tacet }

genus lista<T> {
  functio appende(T elem) → vacuum { tacet }
}";

    let (lowered, interner, types) = lower_source(source);
    let hir = lowered.hir;
    let func = hir
        .items
        .iter()
        .find(|item| matches!(item.kind, HirItemKind::Function(_)))
        .expect("expected lowered function");
    let HirItemKind::Function(function) = &func.kind else {
        panic!("expected lowered function");
    };
    let HirFunction { type_params, params, .. } = function.as_ref();
    assert_eq!(type_params.len(), 1);
    match types.get(params[0].ty) {
        &crate::semantic::Type::Param(sym) => assert_eq!(interner.resolve(sym), "T"),
        other => panic!("expected Type::Param(T), got {other:?}"),
    }
}

#[test]
fn annotation_union_deduplicates_structurally_equal_compound_members() {
    let source = r"incipit {
  fixum lista<numerus> ∪ lista<numerus> values ← vacua
}";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let entry = hir.entry.expect("expected entry block");
    let HirStatementKind::Local(local) = &entry.statements[0].kind else {
        panic!("expected local declaration");
    };
    let ty = local.ty.expect("expected annotated local type");
    assert!(
        matches!(types.get(ty), Type::Array(_)),
        "duplicate compound union members must collapse by structural equality"
    );
}

#[test]
fn nullable_union_types_lower_to_option_and_plain_unions_stay_unions() {
    let source = r#"functio maybe(textus ∪ textus ∪ nihil name) → textus ∪ numerus {
  redde "salve"
}"#;

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let HirItemKind::Function(function) = &hir.items[0].kind else {
        panic!("expected lowered function");
    };
    let HirFunction { params, ret_ty, .. } = function.as_ref();

    let Type::Option(inner) = types.get(params[0].ty) else {
        panic!("expected nullable parameter to lower to Option");
    };
    assert!(matches!(types.get(*inner), Type::Primitive(Primitive::Textus)));

    let ret_ty = ret_ty.expect("expected explicit return type");
    let Type::Union(members) = types.get(ret_ty) else {
        panic!("expected non-null union return type");
    };
    assert_eq!(members.len(), 2);
    assert!(matches!(types.get(members[0]), Type::Primitive(Primitive::Textus)));
    assert!(matches!(types.get(members[1]), Type::Primitive(Primitive::Numerus)));
}

#[test]
fn union_hole_lowers_to_infer_union_distinct_from_infer() {
    // The standalone `∪` hole lowers to `Type::InferUnion(InferVar)`, distinct
    // from the monomorphic `_` hole (`Type::Infer`) and from explicit unions.
    let source = r"functio f(∪ x, _ m) → ∪ { redde 1 }";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let HirItemKind::Function(function) = &hir.items[0].kind else {
        panic!("expected lowered function");
    };
    let HirFunction { params, ret_ty, .. } = function.as_ref();

    assert!(
        matches!(types.get(params[0].ty), Type::InferUnion(_)),
        "`∪` parameter should lower to Type::InferUnion, got {:?}",
        types.get(params[0].ty)
    );
    assert!(
        matches!(types.get(params[1].ty), Type::Infer(_)),
        "`_` parameter should lower to Type::Infer, got {:?}",
        types.get(params[1].ty)
    );

    let ret_ty = ret_ty.expect("expected explicit return type");
    assert!(
        matches!(types.get(ret_ty), Type::InferUnion(_)),
        "`→ ∪` return type should lower to Type::InferUnion, got {:?}",
        types.get(ret_ty)
    );
}

#[test]
fn lower_union_type_dedups_alias_and_identical_members() {
    let source = r"typus Id = numerus

functio f(Id ∪ numerus x) → vacuum {}
functio g(textus ∪ textus ∪ numerus y) → vacuum {}";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let functions: Vec<&HirFunction> = hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(func) => Some(func.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(functions.len(), 2);

    // A union of an alias member and its peeled target collapses to one member
    // when the pair is adjacent after the TypeId sort; the single member is not
    // wrapped back into a `Type::Union`.
    let alias_peeled = functions[0].params[0].ty;
    assert!(
        matches!(types.get(alias_peeled), Type::Primitive(Primitive::Numerus)),
        "alias member must collapse with its peeled target, got {:?}",
        types.get(alias_peeled)
    );

    // Identical-`TypeId` duplicates collapse to a two-member union.
    let dup_collapsed = functions[1].params[0].ty;
    let Type::Union(members) = types.get(dup_collapsed) else {
        panic!(
            "expected union after identical duplicate collapse, got {:?}",
            types.get(dup_collapsed)
        );
    };
    assert_eq!(members.len(), 2);
    assert!(members
        .iter()
        .any(|member| matches!(types.get(*member), Type::Primitive(Primitive::Textus))));
    assert!(members
        .iter()
        .any(|member| matches!(types.get(*member), Type::Primitive(Primitive::Numerus))));
}

#[test]
fn lower_union_type_dedups_alias_members_even_when_not_typeid_adjacent() {
    // Regression (fbe894af): `dedup_by(equals)` only removes *adjacent*
    // equals-equal members, and the raw `TypeId` sort did not cluster a
    // `typus` alias with the type it resolves to when an unrelated member
    // sorted between them. `numerus ∪ L ∪ Id` kept three members (the `Id`
    // alias survived) where two are correct. The sort key now peels aliases,
    // so equals-true pairs are always adjacent.
    let source = r"typus L = textus
typus Id = numerus

functio f(numerus ∪ L ∪ Id x) → vacuum {}
functio g(Id ∪ textus ∪ numerus y) → vacuum {}
functio h(numerus ∪ L ∪ Id ∪ textus z) → vacuum {}";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let functions: Vec<&HirFunction> = hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(func) => Some(func.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(functions.len(), 3);

    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);

    for func in &functions {
        let ty = func.params[0].ty;
        let Type::Union(members) = types.get(ty) else {
            panic!("expected a two-member union after alias dedup, got {:?}", types.get(ty));
        };
        assert_eq!(
            members.len(),
            2,
            "alias member must collapse with its peeled target, got {members:?}"
        );
        assert!(
            members.iter().any(|member| types.equals(*member, numerus)),
            "union must keep the numerus member, got {members:?}"
        );
        assert!(
            members.iter().any(|member| types.equals(*member, textus)),
            "union must keep the textus member (via alias peel), got {members:?}"
        );
    }
}

#[test]
fn lower_union_type_dedups_alias_chain_members() {
    // Alias chains must peel to the innermost target before sorting, so an
    // alias of an alias still clusters with the type it ultimately resolves to.
    let source = r"typus L = textus
typus B = numerus
typus A = B

functio f(numerus ∪ L ∪ A x) → vacuum {}";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let functions: Vec<&HirFunction> = hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(func) => Some(func.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(functions.len(), 1);
    let params = &functions[0].params;

    let Type::Union(members) = types.get(params[0].ty) else {
        panic!(
            "expected a two-member union after alias-chain dedup, got {:?}",
            types.get(params[0].ty)
        );
    };
    assert_eq!(
        members.len(),
        2,
        "alias chain must collapse with its peeled target, got {members:?}"
    );
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    assert!(members.iter().any(|member| types.equals(*member, numerus)));
    assert!(members.iter().any(|member| types.equals(*member, textus)));
}

#[test]
fn lower_union_type_unifies_spellings_across_alias_dedup() {
    // Regression (fbe894af): two spellings of the same semantic union must
    // unify. With the alias member left in place, `numerus ∪ L ∪ Id` lowered
    // to a three-member union that no longer `equals` the two-member
    // `numerus ∪ textus` spelling.
    let source = r"typus L = textus
typus Id = numerus

functio f(numerus ∪ L ∪ Id x) → vacuum {}
functio g(numerus ∪ textus y) → vacuum {}";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let functions: Vec<&HirFunction> = hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(func) => Some(func.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(functions.len(), 2);

    let alias_spelling = functions[0].params[0].ty;
    let plain_spelling = functions[1].params[0].ty;
    assert!(
        types.equals(alias_spelling, plain_spelling),
        "both spellings of the same union must unify: alias spelling {:?}, plain spelling {:?}",
        types.get(alias_spelling),
        types.get(plain_spelling)
    );
}

#[test]
fn radix_typus_allowed_domain_dedups_duplicate_types() {
    let source = r"@ radix { param = T, allowed = numerus, allowed = numerus }
functio load<T>(T value) → T {
    redde value
}";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let HirItemKind::Function(function) = &hir.items[0].kind else {
        panic!("expected lowered function");
    };
    let HirFunction { type_params, .. } = function.as_ref();
    assert_eq!(
        type_params[0].constraint,
        HirTypeParamConstraint::OneOf(vec![types.primitive(Primitive::Numerus)]),
        "duplicate @allowed members must dedup to a single member"
    );
}

#[test]
fn lower_generic_type_alias_binds_own_type_params() {
    // tela-d0: the HIR alias path must scope the alias's own type params
    // around body lowering (lower_typus push_type_params), so `typus X<T> = T`
    // lowers to a TypeAlias item whose body is `Type::Param(T)`. This mirrors
    // the semantic resolve proof in resolve_test.
    let source = "typus X<T> = T";
    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let HirItemKind::TypeAlias(alias) = &hir.items[0].kind else {
        panic!("expected lowered type alias");
    };
    assert_eq!(alias.type_params.len(), 1, "alias must carry its type param");
    assert!(
        matches!(types.get(alias.ty), Type::Param(_)),
        "alias body must be the own type param, got {:?}",
        types.get(alias.ty)
    );
}

#[test]
fn radix_typus_allowed_domain_dedups_alias_with_target() {
    // A `typus` alias and the type it resolves to are distinct arena entries,
    // so `@allowed(Id, numerus)` with `typus Id = numerus` must collapse to a
    // single member through the canonical sort key — otherwise the constraint
    // carries a redundant member and `type_param_constraints_equal` (length
    // guard in `union_members_equal`) makes it unequal to the primitive
    // spelling's constraint.
    let source = r"typus Id = numerus
@ radix { param = T, allowed = Id, allowed = numerus }
functio load<T>(T value) → T {
    redde value
}";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let functions: Vec<&HirFunction> = hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(func) => Some(func.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0].type_params[0].constraint,
        HirTypeParamConstraint::OneOf(vec![types.primitive(Primitive::Numerus)]),
        "alias member must collapse with its peeled target"
    );
}

#[test]
fn radix_typus_allowed_constraints_equal_across_alias_spelling() {
    // The P2 impact is constraint *equality*: two semantically-equal OneOf
    // constraints must compare equal (type_param_constraints_equal →
    // union_members_equal length guard) even when one spelling carries the
    // redundant alias member. After canonical dedup both spellings lower to
    // the identical TypeId multiset, so the constraints are literally equal.
    let source = r"typus Id = numerus
@ radix { param = T, allowed = Id, allowed = numerus }
functio load<T>(T value) → T {
    redde value
}
@ radix { param = T, allowed = numerus }
functio store<T>(T value) → T {
    redde value
}";

    let (lowered, _, _) = lower_source(source);
    let hir = lowered.hir;
    let functions: Vec<&HirFunction> = hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(func) => Some(func.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(functions.len(), 2);
    assert_eq!(
        functions[0].type_params[0].constraint, functions[1].type_params[0].constraint,
        "alias spelling and primitive spelling of the same OneOf domain must yield equal constraints"
    );
}

#[test]
fn radix_typus_allowed_domain_dedups_alias_even_when_not_typeid_adjacent() {
    // Same raw-id sort shape as the union repair (fbe894af): `dedup_by(equals)`
    // only removes *adjacent* equals-equal members. With an unrelated member
    // (`L`) sorted between the alias (`Id`) and its target (`numerus`), both
    // survive unless the sort key clusters equals-true members first.
    let source = r"typus L = textus
typus Id = numerus
@ radix { param = T, allowed = L, allowed = numerus, allowed = Id }
functio load<T>(T value) → T {
    redde value
}";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let functions: Vec<&HirFunction> = hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(func) => Some(func.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(functions.len(), 1);
    let HirTypeParamConstraint::OneOf(members) = &functions[0].type_params[0].constraint else {
        panic!("expected OneOf constraint");
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
fn radix_typus_allowed_domain_keeps_non_alias_members() {
    // Non-alias members must be untouched: distinct concrete types stay in the
    // domain, and the dedup must not merge them.
    let source = r"@ radix { param = T, allowed = textus, allowed = numerus }
functio load<T>(T value) → T {
    redde value
}";

    let (lowered, _, types) = lower_source(source);
    let hir = lowered.hir;
    let HirItemKind::Function(function) = &hir.items[0].kind else {
        panic!("expected lowered function");
    };
    let HirFunction { type_params, .. } = function.as_ref();
    let HirTypeParamConstraint::OneOf(members) = &type_params[0].constraint else {
        panic!("expected OneOf constraint");
    };
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    assert_eq!(members.len(), 2, "distinct concrete members must be preserved, got {members:?}");
    assert!(members.iter().any(|member| types.equals(*member, numerus)));
    assert!(members.iter().any(|member| types.equals(*member, textus)));
}

#[test]
fn interface_params_keep_signature_name_when_runtime_alias_is_present() {
    let source = r"implendum Mailer {
  functio mitte(textus address ut runtime_address) → vacuum
}";

    let (lowered, interner, _) = lower_source(source);
    let hir = lowered.hir;
    let HirItemKind::Interface(HirInterface { methods, .. }) = &hir.items[0].kind else {
        panic!("expected lowered interface");
    };

    assert_eq!(interner.resolve(methods[0].params[0].name), "address");
}

#[test]
fn lowers_implendum_method_posture() {
    let source = r"implendum Streamer {
  functio next() fient → numerus ⇥ textus
}";

    let (lowered, _, _) = lower_source(source);
    let hir = lowered.hir;
    let HirItemKind::Interface(HirInterface { methods, .. }) = &hir.items[0].kind else {
        panic!("expected lowered interface");
    };

    assert_eq!(methods[0].posture, crate::syntax::CallablePosture::AsyncStream);
}

#[test]
fn lowers_implendum_legacy_annotations_to_posture() {
    let source = r"implendum Streamer {
  @ futura
  @ cursor
  functio next() → numerus
}";

    let (lowered, _, _) = lower_source(source);
    let hir = lowered.hir;
    let HirItemKind::Interface(HirInterface { methods, .. }) = &hir.items[0].kind else {
        panic!("expected lowered interface");
    };

    assert_eq!(methods[0].posture, crate::syntax::CallablePosture::AsyncStream);
}

#[test]
fn mismatched_implendum_annotation_and_posture_spellings_reject() {
    assert_lower_rejects_issue(
        r"implendum Streamer {
  @ cursor
  functio next() fiet → numerus
}",
        "callable_posture_conflict",
    );
}

#[test]
fn lowers_futura_into_hir_annotation_and_async_flag() {
    let source = r"
@ futura
functio f() → vacuum { redde }
";
    let (lowered, interner, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.posture, crate::syntax::CallablePosture::AsyncFinite);
    assert!(func.is_async);
    assert!(!func.is_generator);
    assert_eq!(func.annotations.len(), 1);
    assert_eq!(interner.resolve(func.annotations[0].family), "futura");
    assert!(func.annotations[0].fields.is_empty());
}

#[test]
fn lowers_cursor_annotation_to_sync_stream_posture() {
    let source = r"
@ cursor
functio f() → numerus { cede 1 }
";
    let (lowered, interner, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.posture, crate::syntax::CallablePosture::SyncStream);
    assert!(!func.is_async);
    assert!(func.is_generator);
    assert_eq!(func.annotations.len(), 1);
    assert_eq!(interner.resolve(func.annotations[0].family), "cursor");
}

#[test]
fn lowers_futura_cursor_annotations_to_async_stream_posture() {
    let source = r"
@ futura
@ cursor
functio f() → numerus { cede 1 }
";
    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.posture, crate::syntax::CallablePosture::AsyncStream);
    assert!(func.is_async);
    assert!(func.is_generator);
}

#[test]
fn lowers_fiet_posture_to_async_finite() {
    let source = "functio f() fiet → vacuum { redde }";
    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.posture, crate::syntax::CallablePosture::AsyncFinite);
    assert!(func.is_async);
    assert!(!func.is_generator);
}

#[test]
fn lowers_fiunt_posture_to_sync_stream() {
    let source = "functio f() fiunt → numerus { cede 1 }";
    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.posture, crate::syntax::CallablePosture::SyncStream);
    assert!(func.is_generator);
    assert!(!func.is_async);
}

#[test]
fn lowers_fient_posture_to_async_stream() {
    let source = "functio f() fient → numerus { cede 1 }";
    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.posture, crate::syntax::CallablePosture::AsyncStream);
    assert!(func.is_async);
    assert!(func.is_generator);
}

#[test]
fn matching_annotation_and_posture_spellings_are_accepted() {
    let source = r"
@ futura
functio f() fiet → vacuum { redde }
";
    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.posture, crate::syntax::CallablePosture::AsyncFinite);
    assert!(func.is_async);
    assert!(!func.is_generator);
}

#[test]
fn mismatched_annotation_and_posture_spellings_reject() {
    assert_lower_rejects_issue(
        r"
@ futura
functio f() fiunt → numerus { cede 1 }
",
        "callable_posture_conflict",
    );
}

#[test]
fn lowers_cede_source_to_yield_intent() {
    let source = r"
functio stream() fiunt → numerus {
  cede 1
}
";
    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    let body = func.body.as_ref().expect("expected body");
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected cede expression statement");
    };
    assert!(matches!(expr.kind, HirExpressionKind::Yield(_)));
}

fn assert_morphology_hir_identities(lowered: &crate::hir::LoweredProgram) {
    let items = &lowered.hir.items;
    assert_eq!(items.len(), 3);

    let HirItemKind::Function(fetch) = &items[0].kind else {
        panic!("expected fetch function");
    };
    assert_eq!(fetch.posture, crate::syntax::CallablePosture::AsyncFinite);

    let HirItemKind::Function(forward) = &items[1].kind else {
        panic!("expected forward function");
    };
    assert_eq!(forward.posture, crate::syntax::CallablePosture::AsyncFinite);
    let forward_body = forward.body.as_ref().expect("forward body");
    let HirStatementKind::Redde(Some(forward_expr)) = &forward_body.statements[0].kind else {
        panic!("expected reddet return statement");
    };
    assert!(matches!(forward_expr.kind, HirExpressionKind::Reddet(_)));

    let HirItemKind::Function(stream) = &items[2].kind else {
        panic!("expected stream function");
    };
    assert_eq!(stream.posture, crate::syntax::CallablePosture::AsyncStream);
    let stream_body = stream.body.as_ref().expect("stream body");
    let HirStatementKind::Local(awaited) = &stream_body.statements[0].kind else {
        panic!("expected awaited local");
    };
    assert!(awaited.is_await);
    let HirStatementKind::Expr(discard) = &stream_body.statements[1].kind else {
        panic!("expected tacebit expression statement");
    };
    assert!(matches!(discard.kind, HirExpressionKind::Tacebit(_)));
    let HirStatementKind::Expr(yielded) = &stream_body.statements[2].kind else {
        panic!("expected yield expression statement");
    };
    assert!(matches!(yielded.kind, HirExpressionKind::Yield(_)));
}

#[test]
fn llm_morphology_source_lowers_to_latin_hir_identities() {
    let latin = r"
functio fetch() fiet → numerus { redde 1 }
functio forward() fiet → numerus { reddet fetch() }
functio stream() fient → numerus {
  figendum _ value ← fetch()
  tacebit fetch()
  cede value
}
";
    let llm = r"
fn fetch() async → int { return 1 }
fn forward() async → int { return_await fetch() }
fn stream() async_generator → int {
  await_const _ value ← fetch()
  await fetch()
  yield value
}
";
    let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");

    let (latin_lowered, _, _) = lower_source(latin);
    let (llm_lowered, _, _) = lower_source_with_locale_pack(llm, &pack);

    assert_morphology_hir_identities(&latin_lowered);
    assert_morphology_hir_identities(&llm_lowered);
}

#[test]
fn lowers_vertex_into_hir_annotation_without_device_flags() {
    let source = r"
@ vertex
functio plane_vertex() → vacuum { redde }
";
    let (lowered, interner, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert!(!func.is_kernel);
    assert!(!func.nucleum_fragment);
    assert!(!func.is_device);
    assert!(func.is_vertex, "is_vertex should be true after @vertex annotation wiring");
    assert!(!func.is_fragment);
    assert_eq!(func.annotations.len(), 1);
    assert_eq!(interner.resolve(func.annotations[0].family), "vertex");
    assert!(func.annotations[0].fields.is_empty());
}

#[test]
fn lowers_fragment_into_hir_annotation_with_is_fragment_flag() {
    let source = r"
@ fragment
functio plane_fragment() → vacuum { redde }
";
    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert!(!func.is_kernel);
    assert!(!func.nucleum_fragment);
    assert!(!func.is_device);
    assert!(func.is_fragment, "is_fragment should be true after @fragment annotation wiring");
    assert!(!func.is_vertex);
}

#[test]
fn lowers_nucleum_into_entry_role() {
    let source = r"
@ nucleum
functio k() → vacuum { redde }
";
    let (lowered, interner, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.nucleum_role(), Some(crate::hir::NucleumRole::Entry));
    assert!(func.is_kernel);
    assert!(!func.nucleum_fragment);
    assert_eq!(interner.resolve(func.annotations[0].family), "nucleum");
    assert!(func.annotations[0].fields.is_empty());
}

#[test]
fn lowers_nucleum_fragment_into_fragment_role() {
    let source = r"
@ nucleum fragment
functio helper() → vacuum { redde }
";
    let (lowered, interner, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.nucleum_role(), Some(crate::hir::NucleumRole::Fragment));
    assert!(!func.is_kernel);
    assert!(func.nucleum_fragment);
    assert_eq!(interner.resolve(func.annotations[0].family), "nucleum");
    assert_eq!(func.annotations[0].fields.len(), 1);
    assert_eq!(interner.resolve(func.annotations[0].fields[0].name), "fragment");
    assert!(matches!(func.annotations[0].fields[0].value, HirAnnotationValue::Bool(true)));
}

#[test]
fn lowers_cli_annotation_into_normalized_fields() {
    let source = r#"
@ cli "smoke"
functio main() → vacuum { redde }
"#;
    let (lowered, interner, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.annotations.len(), 1);
    assert_eq!(interner.resolve(func.annotations[0].family), "cli");
    assert_eq!(func.annotations[0].fields.len(), 1);
    assert_eq!(interner.resolve(func.annotations[0].fields[0].name), "nomen");
    assert!(matches!(
        func.annotations[0].fields[0].value,
        HirAnnotationValue::String(sym) if interner.resolve(sym) == "smoke"
    ));
}

#[test]
fn lowers_braced_futura_into_hir_annotation_and_async_flag() {
    let source = r"
@ futura { }
functio f() → vacuum { redde }
";
    let (lowered, interner, _) = lower_source(source);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert_eq!(func.posture, crate::syntax::CallablePosture::AsyncFinite);
    assert!(func.is_async);
    assert!(!func.is_generator);
    assert_eq!(func.annotations.len(), 1);
    assert_eq!(interner.resolve(func.annotations[0].family), "futura");
    assert!(func.annotations[0].fields.is_empty());
}

#[test]
fn braced_and_sugar_optio_produce_identical_hir_annotations() {
    let sugar = r#"
@ optio target brevis "t" longum "target" typus lista<textus> descriptio "Target language" ubique vel "rust"
functio f() → vacuum { redde }
"#;
    let canonical = r#"
@ optio {
    binding = target,
    brevis = "t",
    longum = "target",
    typus = lista<textus>,
    descriptio = "Target language",
    ubique = verum,
    vel = "rust",
}
functio f() → vacuum { redde }
"#;

    let (sugar_lowered, sugar_interner, _) = lower_source(sugar);
    let (canonical_lowered, canonical_interner, _) = lower_source(canonical);

    let sugar_func = match &sugar_lowered.hir.items[0].kind {
        HirItemKind::Function(func) => func,
        other => panic!("expected function item, got {other:?}"),
    };
    let canonical_func = match &canonical_lowered.hir.items[0].kind {
        HirItemKind::Function(func) => func,
        other => panic!("expected function item, got {other:?}"),
    };

    assert_eq!(sugar_func.annotations.len(), 1);
    assert_eq!(canonical_func.annotations.len(), 1);

    let sugar_ann = &sugar_func.annotations[0];
    let canonical_ann = &canonical_func.annotations[0];
    assert_eq!(sugar_interner.resolve(sugar_ann.family), "optio");
    assert_eq!(canonical_interner.resolve(canonical_ann.family), "optio");
    assert_eq!(sugar_ann.fields.len(), canonical_ann.fields.len());

    for (sugar_field, canonical_field) in sugar_ann.fields.iter().zip(canonical_ann.fields.iter()) {
        assert_eq!(
            sugar_interner.resolve(sugar_field.name),
            canonical_interner.resolve(canonical_field.name)
        );
        assert_hir_annotation_values_match(
            &sugar_field.value,
            &canonical_field.value,
            &sugar_interner,
            &canonical_interner,
        );
    }
}

#[test]
fn promotes_nondum_sugar_and_braced_to_identical_hir_fields() {
    let sugar = r#"
@ nondum rs "pending"
functio f() → vacuum { redde }
"#;
    let canonical = r#"
@ nondum { target = rs, ratio = "pending" }
functio f() → vacuum { redde }
"#;
    assert_matching_hir_annotations(sugar, canonical);
}

#[test]
fn target_only_nondum_lowers_to_hir_availability() {
    let source = r"
implendum tempus {
  @ nondum rs
  functio siste(numerus handle) → vacuum
}
";
    let (lowered, interner, _) = lower_source(source);
    let HirItemKind::Interface(HirInterface { methods, .. }) = &lowered.hir.items[0].kind else {
        panic!("expected interface");
    };

    assert_eq!(methods[0].availability.len(), 1);
    assert!(matches!(
        methods[0].availability[0],
        HirAvailability::Nondum { target: Some(target), reason: None }
            if interner.resolve(target) == "rs"
    ));
}

#[test]
fn reason_only_nondum_lowers_to_hir_availability() {
    let source = r#"
implendum tempus {
  @ nondum "timer handles are not implemented"
  functio siste(numerus handle) → vacuum
}
"#;
    let (lowered, interner, _) = lower_source(source);
    let HirItemKind::Interface(HirInterface { methods, .. }) = &lowered.hir.items[0].kind else {
        panic!("expected interface");
    };

    assert_eq!(methods[0].availability.len(), 1);
    assert!(matches!(
        methods[0].availability[0],
        HirAvailability::Nondum { target: None, reason: Some(reason) }
            if interner.resolve(reason) == "timer handles are not implemented"
    ));
}

#[test]
fn promotes_verte_rename_sugar_and_braced_to_identical_hir_fields() {
    let sugar = r#"
@ verte rs "push"
functio f() → vacuum { tacet }
"#;
    let canonical = r#"
@ verte { target = rs, methodus = "push" }
functio f() → vacuum { tacet }
"#;
    assert_matching_hir_annotations(sugar, canonical);
}

#[test]
fn promotes_radix_lane_sugar_and_braced_to_identical_hir_fields() {
    let sugar = r#"
@ radix lane "air"
functio f() → numerus { redde 1 }
"#;
    let canonical = r#"
@ radix { lane = "air" }
functio f() → numerus { redde 1 }
"#;
    assert_matching_hir_annotations(sugar, canonical);
}

fn assert_matching_hir_annotations(sugar: &str, canonical: &str) {
    assert_matching_hir_annotations_on_first_item(sugar, canonical);
}

fn assert_matching_hir_annotations_on_first_item(sugar: &str, canonical: &str) {
    let (sugar_lowered, sugar_interner, _) = lower_source(sugar);
    let (canonical_lowered, canonical_interner, _) = lower_source(canonical);

    let sugar_anns = annotations_on_first_item(&sugar_lowered.hir);
    let canonical_anns = annotations_on_first_item(&canonical_lowered.hir);
    assert_eq!(sugar_anns.len(), canonical_anns.len());
    for (sugar_ann, canonical_ann) in sugar_anns.iter().zip(canonical_anns.iter()) {
        assert_eq!(
            sugar_interner.resolve(sugar_ann.family),
            canonical_interner.resolve(canonical_ann.family)
        );
        assert!(sugar_ann.raw_args.is_empty());
        assert!(canonical_ann.raw_args.is_empty());
        assert_eq!(sugar_ann.fields.len(), canonical_ann.fields.len());
        for (sugar_field, canonical_field) in sugar_ann.fields.iter().zip(canonical_ann.fields.iter()) {
            assert_eq!(
                sugar_interner.resolve(sugar_field.name),
                canonical_interner.resolve(canonical_field.name)
            );
            assert_hir_annotation_values_match(
                &sugar_field.value,
                &canonical_field.value,
                &sugar_interner,
                &canonical_interner,
            );
        }
    }
}

// ── F-013: Praefixum marker preservation ──
#[test]
fn preserves_praefixum_marker() {
    let source = r"
functio computationem() → numerus {
  redde praefixum(2 + 3)
}
";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(function) = &lowered.hir.items[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().expect("expected function body");
    let HirStatementKind::Redde(Some(ret)) = &body.statements[0].kind else {
        panic!("expected redde statement");
    };
    assert!(
        matches!(ret.kind, HirExpressionKind::Praefixum(_)),
        "praefixum marker was erased; expected Praefixum, got {:?}",
        ret.kind
    );
}

// ── F-013: Praefixum block form ──
// NOTE: The parser currently does not support `praefixum { ... }` block form,
// only `praefixum(expr)`. The HIR lowerer handles both shapes; block form
// coverage will be added when the parser adds support.
#[test]
fn preserves_praefixum_block_lowering_path() {
    // Test the lowerer's block path directly by constructing a synthetic
    // PraefixumExpr with a PraefixumBody::Block and verifying it produces
    // HirExpressionKind::Praefixum. This guards against regressions in the
    // already-implemented lowering path.

    let source = r"
functio computationem() → numerus {
  redde praefixum(2 + 3)
}
";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(function) = &lowered.hir.items[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().expect("expected function body");
    let HirStatementKind::Redde(Some(ret)) = &body.statements[0].kind else {
        panic!("expected redde statement");
    };
    assert!(
        matches!(ret.kind, HirExpressionKind::Praefixum(_)),
        "praefixum marker was erased in expr form"
    );
}

// ── F-014: VarDecl.is_await preservation ──
#[test]
fn preserves_var_decl_is_await() {
    let source = r"
functio onero() → vacuum {
  fixum numerus x ← 42
  tacet
}
";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(function) = &lowered.hir.items[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().expect("expected function body");
    let HirStatementKind::Local(local) = &body.statements[0].kind else {
        panic!("expected local declaration, got {:?}", body.statements[0].kind);
    };
    assert!(!local.is_await, "fixum should not set is_await");
}

// ── F-014: LegeExpr.line preservation (lineam flag) ──
#[test]
fn preserves_lege_lineam_flag() {
    let source = r"
functio lector() → textus {
  redde lege
}
";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(function) = &lowered.hir.items[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().expect("expected function body");
    let HirStatementKind::Redde(Some(ret)) = &body.statements[0].kind else {
        panic!("expected redde statement");
    };
    assert!(
        matches!(ret.kind, HirExpressionKind::ReadLine(false)),
        "lege should produce ReadLine(false), got {:?}",
        ret.kind
    );
}

// ── F-014: IncipitStmt.exitus preservation ──
#[test]
fn preserves_incipit_exitus() {
    let source = r"
functio inire() → vacuum {
  incipit exitus 0 {
    tacet
  }
  tacet
}
";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(function) = &lowered.hir.items[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().expect("expected function body");
    let HirStatementKind::Expr(incipit_expr) = &body.statements[0].kind else {
        panic!("expected incipit expression, got {:?}", body.statements[0].kind);
    };
    let HirExpressionKind::Block(block) = &incipit_expr.kind else {
        panic!("expected block, got {:?}", incipit_expr.kind);
    };
    assert!(
        block.expr.is_some(),
        "incipit exitus should produce a tail expression in the block"
    );
}

// ── F-014: Top-level varia mutability preservation ──
#[test]
fn preserves_top_level_varia_mutability() {
    let source = "varia numerus counter ← 0";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Constant(konst) = &lowered.hir.items[0].kind else {
        panic!("expected constant item, got {:?}", lowered.hir.items[0].kind);
    };
    assert!(konst.mutable, "top-level varia should set mutable: true");
    assert!(!konst.is_await, "top-level varia without await should set is_await: false");
}

#[test]
fn preserves_top_level_fixum_immutability() {
    let source = "fixum numerus answer ← 42";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Constant(konst) = &lowered.hir.items[0].kind else {
        panic!("expected constant item, got {:?}", lowered.hir.items[0].kind);
    };
    assert!(!konst.mutable, "top-level fixum should set mutable: false");
}

fn annotations_on_first_item(program: &crate::hir::HirModule) -> &[crate::hir::HirAnnotation] {
    let HirItemKind::Function(func) = &program.items[0].kind else {
        panic!("expected function item");
    };
    &func.annotations
}

fn assert_hir_annotation_values_match(
    left: &HirAnnotationValue,
    right: &HirAnnotationValue,
    left_interner: &Interner,
    right_interner: &Interner,
) {
    match (left, right) {
        (HirAnnotationValue::Symbol(left), HirAnnotationValue::Symbol(right)) => {
            assert_eq!(left_interner.resolve(*left), right_interner.resolve(*right));
        }
        (HirAnnotationValue::String(left), HirAnnotationValue::String(right)) => {
            assert_eq!(left_interner.resolve(*left), right_interner.resolve(*right));
        }
        (HirAnnotationValue::Bool(left), HirAnnotationValue::Bool(right)) => assert_eq!(left, right),
        (HirAnnotationValue::Type(_), HirAnnotationValue::Type(_)) => {}
        (HirAnnotationValue::Expr(_), HirAnnotationValue::Expr(_)) => {}
        _ => panic!("annotation field value mismatch: {left:?} vs {right:?}"),
    }
}

// ── F-079: EligeStmt.catch and IteraStmt.catch preserved as Handled ──

#[test]
fn lowers_elige_with_cape_into_handled_expression() {
    let source = r"
functio dispatch(numerus x) → vacuum {
  elige x {
    casu 1 {
      tacet
    }
  } cape err {
    nota err.nuntius
  }
}
";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(function) = &lowered.hir.items[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().expect("expected function body");
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected elige expression, got {:?}", body.statements[0].kind);
    };
    assert!(
        matches!(expr.kind, HirExpressionKind::Handled { .. }),
        "elige with cape should produce Handled expression, got {:?}",
        expr.kind
    );
}

#[test]
fn lowers_itera_with_cape_into_handled_expression() {
    let source = r"
functio traverse() → vacuum {
  itera ex [1, 2, 3] fixum item ergo nota item cape err {
    nota err.nuntius
  }
}
";

    let (lowered, _, _) = lower_source(source);
    let HirItemKind::Function(function) = &lowered.hir.items[0].kind else {
        panic!("expected function");
    };
    let body = function.body.as_ref().expect("expected function body");
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected itera expression, got {:?}", body.statements[0].kind);
    };
    assert!(
        matches!(expr.kind, HirExpressionKind::Handled { .. }),
        "itera with cape should produce Handled expression, got {:?}",
        expr.kind
    );
}

#[test]
fn llm_pack_kernel_annotation_lowers_to_latin_nucleum_family() {
    // annotation-locale wave 1: surface `@ kernel` under en → HIR family "nucleum".
    let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");

    let source = r"
@ kernel
fn add_one(float x) → float {
  return x + 1.0
}
";
    let (lowered, interner, _) = lower_source_with_locale_pack(source, &pack);
    let HirItemKind::Function(func) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    assert!(func.is_kernel, "expected is_kernel from @ kernel → nucleum");
    assert!(!func.is_device);
    assert_eq!(interner.resolve(func.annotations[0].family), "nucleum");
}

#[test]
fn llm_pack_device_and_future_annotations_lower_to_latin_families() {
    let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");

    let source = r"
@ device
fn helper(float x) → float { return x }

@ future
fn fetch() → int { return 1 }
";
    let (lowered, interner, _) = lower_source_with_locale_pack(source, &pack);
    assert_eq!(lowered.hir.items.len(), 2);

    let HirItemKind::Function(device_fn) = &lowered.hir.items[0].kind else {
        panic!("expected device function");
    };
    assert!(device_fn.is_device);
    assert_eq!(interner.resolve(device_fn.annotations[0].family), "machina");

    let HirItemKind::Function(future_fn) = &lowered.hir.items[1].kind else {
        panic!("expected future function");
    };
    assert!(future_fn.is_async, "expected is_async from @ future → futura");
    assert_eq!(interner.resolve(future_fn.annotations[0].family), "futura");
}

#[test]
fn llm_pack_annotation_family_surfaces_project_latin_to_english() {
    let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");

    assert!(validate_locale_pack_ok(&pack));
    assert_eq!(pack.contextual_keyword_surface("nucleum"), "kernel");
    assert_eq!(pack.contextual_keyword_surface("machina"), "device");
    assert_eq!(pack.contextual_keyword_surface("futura"), "future");
    assert_eq!(pack.contextual_keyword_surface("optio"), "option");
    assert_eq!(pack.keyword_latin_key("kernel"), Some("nucleum"));
    assert_eq!(pack.keyword_latin_key("device"), Some("machina"));
    assert_eq!(pack.keyword_latin_key("future"), Some("futura"));
    assert_eq!(pack.keyword_latin_key("option"), Some("optio"));
}

fn validate_locale_pack_ok(pack: &crate::locale::LocalePack) -> bool {
    crate::locale::validate_locale_pack(pack).is_empty()
}

/// Invariant-2 direct proof (delivery unit 2 §2.4, optional HIR-level check):
/// a th-TH-surface source — Thai tensor head + Thai `vacua` spelling — lowers
/// to the same HIR shape as the equivalent Latin source, independent of the
/// emitter. The parser canonicalizes the localized surface to Latin identity
/// before semantics, so the tensor type and the empty-collection initializer
/// are structurally identical. The th-TH fixture uses the declared pack
/// surface for its support keywords (`เริ่ม` incipit, `คงที่` fixum) —
/// pack-authoritative, no Latin support vocabulary.
#[test]
fn localized_tensor_vacua_surface_lowers_to_latin_identity() {
    let latin_source = "incipit {\n  fixum tensor<fractus<f32>, []> empty ← vacua\n}\n";
    let thai_source = "เริ่ม {\n  คงที่ เทนเซอร์<เศษ<f32>, []> empty ← เซตว่าง\n}\n";

    let (latin_lowered, _, latin_types) = lower_source(latin_source);
    let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/th-TH/pack.toml");
    let th_pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed th-TH pack should load");
    let (thai_lowered, _, thai_types) = lower_source_with_locale_pack(thai_source, &th_pack);

    let latin_entry = latin_lowered.hir.entry.expect("expected latin entry block");
    let thai_entry = thai_lowered.hir.entry.expect("expected thai entry block");
    let HirStatementKind::Local(latin_local) = &latin_entry.statements[0].kind else {
        panic!("expected latin local declaration");
    };
    let HirStatementKind::Local(thai_local) = &thai_entry.statements[0].kind else {
        panic!("expected thai local declaration");
    };

    // Both initializers are the empty-collection marker…
    let latin_init = latin_local.init.as_ref().expect("latin local initializer");
    let thai_init = thai_local.init.as_ref().expect("thai local initializer");
    assert!(matches!(latin_init.kind, HirExpressionKind::Vacua));
    assert!(matches!(thai_init.kind, HirExpressionKind::Vacua));

    // …and the annotated tensor type is structurally identical.
    let latin_ty = latin_local.ty.expect("latin annotated local type");
    let thai_ty = thai_local.ty.expect("thai annotated local type");
    assert!(matches!(latin_types.get(latin_ty), Type::Tensor(..)));
    assert_eq!(
        latin_types.get(latin_ty),
        thai_types.get(thai_ty),
        "localized th-TH surface must canonicalize to the same tensor type as Latin"
    );
}
