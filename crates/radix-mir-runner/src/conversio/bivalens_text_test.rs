//! `textus ↦ bivalens` parses exactly the code locale's true/false words and
//! fails on anything else; non-empty is no longer true (operator ruling
//! 2026-10-15, need). The structured conversion code classifies a
//! failure; the prose message stays free.

use crate::host::BufferHost;
use crate::{Runner, RunnerError, Value};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram, MirType,
    MirValidationContext,
};
use radix_runtime_contract::display_tokens::DisplayTokens;
use radix_types::{TypeId, TypeTable};

fn empty_function() -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn english() -> DisplayTokens {
    DisplayTokens {
        true_: "true".to_owned(),
        false_: "false".to_owned(),
        none: "none".to_owned(),
        tuple: "tuple".to_owned(),
    }
}

/// Function 0 is the one running, so `display_tokens()` resolves `tokens` (or
/// falls back to Latin when `tokens` is `None`).
fn convert(
    tokens: Option<&DisplayTokens>,
    text: &str,
    recovery: Option<MirOperand>,
) -> Result<Value, RunnerError> {
    let types = TypeTable::new();
    let interner = Interner::new();
    let mut context = MirValidationContext::new(&types);
    if let Some(tokens) = tokens {
        context.display_tokens.assign(MirFunctionId(0), tokens);
    }
    let program = MirProgram {
        functions: vec![empty_function()],
    };
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.convert_to_bivalens(
        Value::Str(text.to_owned()),
        recovery.as_ref(),
        Span::default(),
    )
}

fn ok(tokens: Option<&DisplayTokens>, text: &str) -> Value {
    convert(tokens, text, None).expect("parse succeeds")
}

fn fails(tokens: Option<&DisplayTokens>, text: &str) -> RunnerError {
    convert(tokens, text, None).expect_err("an unparseable word must fail")
}

#[test]
fn english_words_parse() {
    let tokens = english();
    assert!(matches!(ok(Some(&tokens), "true"), Value::Bool(true)));
    assert!(matches!(ok(Some(&tokens), "false"), Value::Bool(false)));
}

#[test]
fn latin_words_parse_in_a_latin_function() {
    let tokens = DisplayTokens::latin();
    assert!(matches!(ok(Some(&tokens), "verum"), Value::Bool(true)));
    assert!(matches!(ok(Some(&tokens), "falsum"), Value::Bool(false)));
}

#[test]
fn non_words_fail_with_the_conversion_code() {
    let tokens = english();
    for text in ["salve", "", "0"] {
        assert_eq!(
            fails(Some(&tokens), text).code,
            crate::RUNNER_CODE_CONVERSION,
            "{text:?}"
        );
    }
}

#[test]
fn latin_function_rejects_english_words() {
    let tokens = DisplayTokens::latin();
    for text in ["true", "false"] {
        assert_eq!(
            fails(Some(&tokens), text).code,
            crate::RUNNER_CODE_CONVERSION,
            "{text:?}"
        );
    }
}

#[test]
fn recovery_yields_the_default_on_an_unparseable_word() {
    let tokens = english();
    let recovery = MirOperand::Constant(MirConstant::Bool(false));
    let value = convert(Some(&tokens), "salve", Some(recovery)).expect("recovery applies");
    assert!(matches!(value, Value::Bool(false)));
}
