//! Parser tests for the kernel declaration head (operator ruling
//! 2026-10-30): `nucleum name<T>(params) [mode] …` parses as the same
//! function declaration the `@ nucleum` annotation form marks, and the
//! head's mode synthesizes the annotation the form authors.
//!
//! Identity assertions target the structured `issue`, never rendered prose.

use crate::test_support::{assert_parse_error_issue_la, parse_ok_la};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{AnnotationKind, ClassMemberKind, KernelTier, Stmt, StmtKind};

/// English surface for the head spellings: the registered keyword rows the
/// installed `en` pack declares for the kernel family.
struct EnKernelHeadPack;

impl KeywordPack for EnKernelHeadPack {
    fn pack_id(&self) -> &'static str {
        "en"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "fn" => Some("functio"),
            "kernel" => Some("nucleum"),
            "relaxed" => Some("laxus"),
            "unsafe" => Some("intutus"),
            "fragment" => Some("fragment"),
            "public" => Some("publica"),
            _ => None,
        }
    }

    fn keyword_spelling(&self, _latin_key: &str) -> Option<&str> {
        None
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

/// The single nucleum annotation riding a statement, with its parsed shape.
fn nucleum_annotation(stmt: &Stmt) -> (bool, KernelTier) {
    let [annotation] = &stmt.annotations[..] else {
        panic!(
            "expected exactly one annotation on the kernel declaration, got {}",
            stmt.annotations.len()
        );
    };
    match &annotation.kind {
        AnnotationKind::Nucleum(nucleum) => (nucleum.is_fragment, nucleum.tier),
        other => panic!("expected a synthesized nucleum annotation, got {other:?}"),
    }
}

fn single_stmt(source: &str) -> crate::ParseResult {
    let result = parse_ok_la(source);
    assert_eq!(
        result.program.as_ref().expect("program").statements.len(),
        1,
        "expected one statement"
    );
    result
}
#[test]
fn kernel_head_parses_as_function_with_synthesized_marker() {
    let result = single_stmt("nucleum k(f32 x) → f32 { redde x }\n");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("a kernel head declares a function");
    };
    assert_eq!(program.statements[0].annotations.len(), 1);
    assert_eq!(
        nucleum_annotation(&program.statements[0]),
        (false, KernelTier::Default)
    );
    assert!(
        func.body.is_some(),
        "the head form takes a block like functio"
    );
}

#[test]
fn kernel_head_mode_words_carry_tier_and_fragment() {
    let tier = |source: &str| {
        let result = single_stmt(source);
        let program = result.program.as_ref().expect("program");
        nucleum_annotation(&program.statements[0])
    };
    assert_eq!(
        tier("nucleum k(f32 x) laxus → f32 { redde x }\n"),
        (false, KernelTier::Relaxed)
    );
    assert_eq!(
        tier("nucleum k(f32 x) intutus → f32 { redde x }\n"),
        (false, KernelTier::Unsafe)
    );
    assert_eq!(
        tier("nucleum k(f32 x) fragment → vacuum { }\n"),
        (true, KernelTier::Default)
    );
}

#[test]
fn kernel_head_takes_generic_parameters_and_modifiers_before_the_mode() {
    let result = single_stmt(
        "nucleum k<magnitudo N>(tensor<f32, [N]> xs) errata e laxus → f32 ⇥ textus { redde 1.0 }\n",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("a kernel head declares a function");
    };
    assert_eq!(func.type_params.len(), 1);
    assert_eq!(func.modifiers.len(), 1);
    assert_eq!(
        nucleum_annotation(&program.statements[0]),
        (false, KernelTier::Relaxed)
    );
}

#[test]
fn kernel_head_stacks_authored_annotations_behind_the_marker() {
    let result = single_stmt("@ publica\nnucleum k(f32 x) → f32 { redde x }\n");
    let program = result.program.as_ref().expect("program");
    let annotations = &program.statements[0].annotations;
    assert_eq!(annotations.len(), 2, "marker plus authored visibility");
    assert!(matches!(annotations[0].kind, AnnotationKind::Nucleum(_)));
    assert!(matches!(annotations[1].kind, AnnotationKind::Publica));
}

#[test]
fn kernel_head_parses_a_bodyless_nondum_declaration() {
    single_stmt("@ nondum\nnucleum k(f32 x) → f32\n");
}

#[test]
fn both_heads_on_one_declaration_are_refused_by_name() {
    assert_parse_error_issue_la(
        "@ nucleum\nnucleum k(f32 x) → f32 { redde x }\n",
        "nucleum_double_head",
    );
    assert_parse_error_issue_la(
        "@ nucleum laxus\nnucleum k(f32 x) → f32 { redde x }\n",
        "nucleum_double_head",
    );
}

#[test]
fn kernel_head_mode_misuse_keeps_the_annotation_form_refusals() {
    assert_parse_error_issue_la(
        "nucleum k(f32 x) laxus intutus → f32 { redde x }\n",
        "duplicate_nucleum_tier",
    );
    assert_parse_error_issue_la(
        "nucleum k(f32 x) celer → f32 { redde x }\n",
        "unknown_nucleum_tier",
    );
    assert_parse_error_issue_la(
        "nucleum k(f32 x) fragment laxus → vacuum { }\n",
        "nucleum_tier_on_fragment",
    );
    assert_parse_error_issue_la(
        "nucleum k(f32 x) laxus fragment → f32 { redde x }\n",
        "nucleum_tier_on_fragment",
    );
}

#[test]
fn kernel_head_fragment_mode_conflicts_with_graphics_fragment_annotation() {
    assert_parse_error_issue_la(
        "@ fragment\nnucleum k(f32 x) fragment → vacuum { }\n",
        "nucleum_fragment_conflicts_with_fragment",
    );
}

#[test]
fn conversion_annotation_refuses_the_kernel_head_by_name() {
    assert_parse_error_issue_la(
        "@ conversio\nnucleum k(f32 x) → f32 { redde x }\n",
        "conversio_kernel_head",
    );
}

#[test]
fn kernel_head_claims_only_a_declaration_name_shape() {
    // `nucleum(…)` is a call and `nucleum.f` a member access: without a
    // declaration name behind it the word stays expression material.
    for source in ["nucleum(3)\n", "nucleum.f\n"] {
        let result = parse_ok_la(source);
        let program = result.program.as_ref().expect("program");
        assert!(
            !matches!(program.statements[0].kind, StmtKind::Func(_)),
            "{source}: must not claim the kernel head"
        );
        assert!(
            program.statements[0].annotations.is_empty(),
            "{source}: claims no marker"
        );
    }
}

#[test]
fn kernel_head_parses_as_a_genus_method_marker() {
    let result = parse_ok_la("genus C {\n    nucleum m(f32 x) → f32 { redde x }\n}\n");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected a genus declaration");
    };
    let [member] = &class.members[..] else {
        panic!("expected one member");
    };
    assert!(matches!(member.kind, ClassMemberKind::Method(_)));
    assert_eq!(member.annotations.len(), 1);
    assert!(matches!(
        member.annotations[0].kind,
        AnnotationKind::Nucleum(_)
    ));
}

#[test]
fn english_kernel_head_spellings_parse() {
    let parse = |source: &str| crate::parse(lex_with_locale_pack(source, &EnKernelHeadPack));
    let result = parse("@ public\nkernel k(f32 x) relaxed → f32 { return x }\n");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    let stmt = &program.statements[0];
    let AnnotationKind::Nucleum(nucleum) = &stmt.annotations[0].kind else {
        panic!("expected a synthesized nucleum marker under the en surface");
    };
    assert!(matches!(stmt.kind, StmtKind::Func(_)));
    assert_eq!(nucleum.tier, KernelTier::Relaxed);
    assert!(!nucleum.is_fragment);
}

#[test]
fn latin_kernel_head_word_stays_an_identifier_under_the_en_surface() {
    let parse = |source: &str| crate::parse(lex_with_locale_pack(source, &EnKernelHeadPack));
    let result = parse("nucleum k(f32 x) → f32 { return x }\n");
    assert!(
        !result.errors.is_empty(),
        "an undeclared Latin spelling must not claim the head under a pack surface"
    );
}
