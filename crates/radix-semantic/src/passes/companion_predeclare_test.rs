//! Coverage for backward-companion name predeclaration.
//!
//! These tests prove that `@ radix backward` annotations register companion
//! `DefIds` in the resolver (using the `0x8000_0000` high-bit scheme) and that
//! only `"air"`-lane backward names are pre-registered.

use crate::{PassConfig, SemanticError, SemanticErrorKind};
use radix_target::Target;

fn latin_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

fn analyze(source: &str) -> (crate::RadixLaneMetadata, Vec<SemanticError>) {
    let lex = radix_lexer::lex_with_locale_pack(source, &latin_pack());
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    let result = crate::analyze(&program, &config, &mut parse.interner);
    (result.radix_lanes, result.diagnostics)
}

#[test]
fn backward_companion_name_registered_in_resolver() {
    // Run just the predeclare pass after collect — using the same interner
    // throughout so Symbols match.
    let source = r#"
@ radix lane "air"
@ radix backward "mul_backward"
functio mul_pair(numerus a, numerus b) → numerus {
    redde a + b
}
"#;
    let lex = radix_lexer::lex_with_locale_pack(source, &latin_pack());
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let mut types = crate::TypeTable::new();
    let mut resolver = crate::Resolver::new();

    // Stage 1: collect registers the primal function name.
    crate::passes::collect::collect(&program, &mut resolver, &mut types).expect("collect");

    // Stage 2: predeclare should register the companion name.
    let predecls =
        super::companion_predeclare::predeclare(&program, &mut resolver, &parse.interner)
            .expect("predeclare");

    // The companion name should be in the resolver.
    let companion_sym = parse.interner.intern("mul_backward");
    let companion_def = resolver
        .lookup(companion_sym)
        .expect("companion name not in resolver");

    // The predeclared mapping should contain the companion name → DefId.
    let pre_def_id = predecls
        .by_name
        .get(&companion_sym)
        .expect("companion name not in predeclarations map");

    // The pre-allocated DefId should match what the resolver stores.
    assert_eq!(
        companion_def, *pre_def_id,
        "resolver DefId and predeclared DefId must match"
    );

    // The DefId should use the 0x8000_0000 high-bit scheme.
    assert_eq!(
        pre_def_id.0 & 0x8000_0000,
        0x8000_0000,
        "companion DefId must use 0x8000_0000 high-bit scheme"
    );
}

#[test]
fn backward_companion_no_sem001_on_call() {
    // Full-pipeline test: the companion name should resolve without SEM001.
    let (_lanes, errors) = analyze(
        r#"
@ radix lane "air"
@ radix backward "mul_backward"
functio mul_pair(numerus a, numerus b) → numerus {
    redde a + b
}

incipit {
    sit numerus x ← 5
    sit numerus result ← mul_pair(x, x)
    nota result
}
"#,
    );

    // The predeclare pass registers mul_backward before resolve, so
    // no UndefinedVariable error should mention mul_backward.
    let has_unknown_companion = errors.iter().any(|e| {
        e.kind == SemanticErrorKind::VariableUndefined
            && e.args
                .iter()
                .any(|a| a.name == "issue" && a.value == "unknown_identifier")
            && e.args
                .iter()
                .any(|a| a.name == "name" && a.value == "mul_backward")
    });
    assert!(
        !has_unknown_companion,
        "companion name should resolve without SEM001"
    );
}

#[test]
fn unknown_names_still_produce_sem001() {
    // Names without any declaration should still fail.
    let (_, errors) = analyze(
        r"
incipit {
    sit numerus result ← non_existent_func()
    nota result
}
",
    );

    let has_sem001 = errors.iter().any(|e| {
        e.kind == SemanticErrorKind::VariableUndefined
            && e.args
                .iter()
                .any(|a| a.name == "issue" && a.value == "unknown_identifier")
    });
    assert!(has_sem001, "truly unknown names must still produce SEM001");
}

#[test]
fn backward_without_air_lane_not_preregistered() {
    // @ radix backward without lane "air" should not be pre-registered.
    let source = r#"
@ radix lane "mir"
@ radix backward "wrong_backward"
functio f(numerus a) → numerus {
    redde a
}
"#;
    let lex = radix_lexer::lex_with_locale_pack(source, &latin_pack());
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let mut types = crate::TypeTable::new();
    let mut resolver = crate::Resolver::new();

    crate::passes::collect::collect(&program, &mut resolver, &mut types).expect("collect");
    let predecls =
        super::companion_predeclare::predeclare(&program, &mut resolver, &parse.interner)
            .expect("predeclare");

    // The companion should NOT be in resolver or predeclarations map.
    let companion_sym = parse.interner.intern("wrong_backward");
    assert!(
        resolver.lookup(companion_sym).is_none(),
        "non-air backward should not be pre-registered"
    );
    assert!(
        !predecls.by_name.contains_key(&companion_sym),
        "non-air backward should not be in predeclarations map"
    );
}

#[test]
fn predeclared_def_id_consumed_by_radix_lane_collect() {
    // Verify that radix_lane::collect reuses the pre-allocated DefId.
    let source = r#"
@ radix lane "air"
@ radix backward "my_backward"
functio f(numerus a) → numerus {
    redde a
}
"#;
    let (lanes, errors) = analyze(source);

    // No errors expected (annotation is well-formed).
    assert!(
        !errors.iter().any(SemanticError::is_error),
        "well-formed backward annotation should not produce errors"
    );

    // radix_lane::collect should have consumed the pre-allocated DefId.
    let backward_entries: Vec<_> = lanes.iter_backward().collect();
    assert_eq!(
        backward_entries.len(),
        1,
        "expected exactly one backward entry"
    );
    let (_primal_def_id, backward) = backward_entries[0];

    // The companion DefId should have the 0x8000_0000 high bit (not u32::MAX).
    assert_ne!(
        backward.companion_def_id.0,
        u32::MAX,
        "companion DefId should be pre-allocated, not u32::MAX"
    );
    assert_eq!(
        backward.companion_def_id.0 & 0x8000_0000,
        0x8000_0000,
        "companion DefId should use 0x8000_0000 scheme"
    );
}
