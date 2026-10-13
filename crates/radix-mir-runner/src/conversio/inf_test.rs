//! `textus ↦ inf` refuses a digit token past the parse ceiling (ruling 45):
//! the parse is O(n^2), so the token is rejected before any limb work.

use crate::host::BufferHost;
use crate::inf::{RUNNER_INF_PARSE_DIGIT_CEILING, RUNNER_ISSUE_INF_SIZE_EXHAUSTED};
use crate::{Runner, RunnerError, Value};
use radix_lexer::{Interner, Span};
use radix_mir::{MirConversion, MirConversionFlavor, MirProgram, MirType, MirValidationContext};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

fn textus_to_inf(text: String) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let source = types.primitive(Primitive::Textus);
    let target = types.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::Inf));
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let program = MirProgram::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.eval_runtime_convert(
        &MirConversion {
            store: None,
            flavor: MirConversionFlavor::Runtime,
            source_ty: MirType::semantic(source),
            target_ty: MirType::semantic(target),
            params: vec![],
            recovery: None,
            struct_defaults: vec![],
        },
        Value::Str(text),
        Span::default(),
    )
}

#[test]
fn a_digit_token_past_the_parse_ceiling_is_a_resource_fault() {
    let digits = "9".repeat(RUNNER_INF_PARSE_DIGIT_CEILING + 1);
    let error = textus_to_inf(digits.clone()).expect_err("over-long token must be refused");
    assert_eq!(error.issue, RUNNER_ISSUE_INF_SIZE_EXHAUSTED);
    // The sign is not a digit, so a signed token is held to the same bound.
    let error = textus_to_inf(format!("-{digits}")).expect_err("over-long signed token");
    assert_eq!(error.issue, RUNNER_ISSUE_INF_SIZE_EXHAUSTED);
}

#[test]
fn a_digit_token_within_the_ceiling_parses_in_full() {
    let digits = format!("1{}", "0".repeat(999));
    let value = textus_to_inf(digits).expect("a 1000-digit token parses");
    assert!(matches!(value, Value::Magnus(_)), "got {value:?}");
}
