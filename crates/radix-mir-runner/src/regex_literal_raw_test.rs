use super::*;

#[test]
fn regex_literal_keeps_double_backslash_raw() {
    let types = TypeTable::new();
    let program = MirProgram::new();
    let context = MirValidationContext::new(&types);
    let mut interner = Interner::new();
    // Raw three-char body: backslash, backslash, `d` (matches a literal
    // backslash followed by `d`).
    let pattern = interner.intern(r"\\d");
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);

    let constant = MirConstant::Regex {
        pattern,
        flags: None,
    };
    let value = runner
        .eval_constant(&constant, Span::default())
        .expect("raw slash-literal regex body compiles");
    let Value::Regex(regex) = &value else {
        panic!("regex constant evaluates to a regex value");
    };
    assert_eq!(regex.pattern(), r"\\d");
    assert!(
        regex.is_match("\\d"),
        "raw \\\\d matches a literal backslash followed by d"
    );
    assert!(
        !regex.is_match("7"),
        "raw \\\\d must not collapse to the digit class"
    );
}

#[test]
fn regex_literal_reuses_compiled_program_for_same_pattern_and_flags() {
    let types = TypeTable::new();
    let program = MirProgram::new();
    let context = MirValidationContext::new(&types);
    let mut interner = Interner::new();
    let pattern = interner.intern(r"\\d+");
    let mut host = BufferHost::default();
    let runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let constant = MirConstant::Regex {
        pattern,
        flags: None,
    };

    let first = runner
        .eval_constant(&constant, Span::default())
        .expect("regex literal compiles");
    let second = runner
        .eval_constant(&constant, Span::default())
        .expect("same regex literal reuses compiled program");
    let (Value::Regex(first), Value::Regex(second)) = (&first, &second) else {
        panic!("regex constants evaluate to regex values");
    };
    assert!(Rc::ptr_eq(first, second));
}

#[test]
fn regex_literal_flags_prefix_raw_body() {
    let types = TypeTable::new();
    let program = MirProgram::new();
    let context = MirValidationContext::new(&types);
    let mut interner = Interner::new();
    let pattern = interner.intern(r"\\d");
    let flags = interner.intern("i");
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);

    let constant = MirConstant::Regex {
        pattern,
        flags: Some(flags),
    };
    let value = runner
        .eval_constant(&constant, Span::default())
        .expect("raw slash-literal regex body compiles");
    let Value::Regex(regex) = &value else {
        panic!("regex constant evaluates to a regex value");
    };
    assert_eq!(regex.pattern(), r"(?i)\\d");
}

#[test]
fn regex_literal_constant_equals_value_is_raw() {
    let types = TypeTable::new();
    let program = MirProgram::new();
    let context = MirValidationContext::new(&types);
    let mut interner = Interner::new();
    let pattern = interner.intern(r"\\d");
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);

    let constant = MirConstant::Regex {
        pattern,
        flags: None,
    };
    let value = runner
        .eval_constant(&constant, Span::default())
        .expect("raw slash-literal regex body compiles");
    assert!(constant_equals_value(&constant, &value, &interner));
}
